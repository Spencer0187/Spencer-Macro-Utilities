#include <iostream>
#include <string>
#include <vector>
#include <map>
#include <set>           // For reliable main process finding
#include <fstream>       // For reading /proc files
#include <sstream>       // For string manipulation
#include <algorithm>     // For std::min_element
#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/epoll.h>
#include <sys/types.h>
#include <signal.h>
#include <ctime>
#include <linux/uinput.h>
#include <linux/input.h>
#include <cstring>
#include <thread>
#include <chrono>
#include <csignal>
#include <dirent.h>
#include <climits>
#include <cstdio>
#include <pwd.h>
#include <cerrno>
#include <cstdlib>
#include <stdexcept>

#include "shared_mem.h"
#include "keymapping.h"

// --- Globals ---
SharedMemory* shared_data = nullptr;
int uinput_fd = -1;
const char* MM_FILE_PATH = "/tmp/cross_input_shm_file";
bool isbhop = false;
bool isdesync = false;

int bhop_delay = 10;
int desync_itemslot = 5;
// --- Function Prototypes ---
void cleanup(int);
bool interactive_device_detection(std::string& out_keyboard_path, std::string& out_mouse_path);
int setup_uinput_device();
void emit_uinput(int type, int code, int val);
void evdev_reader_thread(const std::string& device_path);
void command_processor_thread();
void special_action_thread();
void fastkey_thread();

pid_t find_main_process_by_name(const std::string& name);
std::vector<pid_t> find_all_processes_by_exes_or_pids(const std::string& name);

// ********************************************************************
// *                  CORE HELPER FUNCTIONS                           *
// ********************************************************************

// Helper to get a process's Parent PID (PPID) from /proc/[pid]/stat
pid_t get_ppid(pid_t pid) {
    std::string path = "/proc/" + std::to_string(pid) + "/stat";
    std::ifstream stat_file(path);
    if (!stat_file) {
        return -1;
    }
    // The 4th field in /proc/pid/stat is the PPID
    std::string unused;
    pid_t ppid = -1;
    stat_file >> unused >> unused >> unused >> ppid;
    return ppid;
}

// Finds the cgroup v2 path for a given PID.
std::string get_cgroup_v2_path(pid_t pid) {
    std::string cgroup_file_path = "/proc/" + std::to_string(pid) + "/cgroup";
    std::ifstream cgroup_file(cgroup_file_path);
    if (!cgroup_file) { return ""; }
    std::string line;
    while (std::getline(cgroup_file, line)) {
        if (line.rfind("0::/", 0) == 0) {
            return "/sys/fs/cgroup" + line.substr(3);
        }
    }
    return "";
}

/**
 * @brief The final hybrid suspension function.
 *
 * It intelligently decides the best suspension method for a given PID.
 * 1.  If the PID belongs to a "safe" application-specific cgroup (e.g., a Flatpak),
 *     it freezes the entire cgroup for a clean, atomic suspend.
 * 2.  Otherwise, it falls back to sending a traditional SIGSTOP/SIGCONT signal
 *     directly to the PID. This handles cases like Firefox running in a generic session.
 */
void hybrid_suspend_or_resume(pid_t pid, bool suspend) {
    std::string cgroup_path = get_cgroup_v2_path(pid);
    const char* action_signal = suspend ? "SIGSTOP" : "SIGCONT";
    const char* action_cgroup = suspend ? "Freezing" : "Thawing";
    int signal_to_send = suspend ? SIGSTOP : SIGCONT;

    // A "safe" cgroup is one explicitly created for an app (e.g., Flatpak, Snap).
    // These typically contain "app-" in their name.
    bool is_safe_app_cgroup = !cgroup_path.empty() && (cgroup_path.find("app-") != std::string::npos);

    if (is_safe_app_cgroup) {
        printf("[Helper] PID %d belongs to a safe app cgroup. %s cgroup: %s\n", pid, action_cgroup, cgroup_path.c_str());
        std::string freeze_file_path = cgroup_path + "/cgroup.freeze";
        std::ofstream freeze_file(freeze_file_path);
        if (freeze_file) {
            freeze_file << (suspend ? "1" : "0");
            if (freeze_file.fail()) {
                perror("[Helper] Cgroup write failed");
            }
        } else {
            perror("[Helper] Cgroup open failed");
        }
    } else {
        printf("[Helper] Cgroup for PID %d is not a specific app scope. Falling back to %s.\n", pid, action_signal);
        if (kill(pid, signal_to_send) != 0) {
            perror(("[Helper] Error sending " + std::string(action_signal)).c_str());
        }
    }
}

void monitor_parent_process(const std::string& parent_process_name) {
    std::cout << "[Monitor] Parent process monitor thread started for '" << parent_process_name << "'." << std::endl;
    
    while (true) {
        // Use pgrep with '-x' for an exact match on the process name.
        // This is a simple and reliable way to check if the process is running.
        std::string command = "pgrep -x " + parent_process_name + " > /dev/null 2>&1";
        
        int result = system(command.c_str());
        
        // system() returns 0 on success (pgrep found the process).
        // A non-zero value means pgrep returned an error, typically 1 for "not found".
        if (result != 0) {
            std::cout << "[Monitor] Parent process '" << parent_process_name << "' not found. Initiating self-shutdown." << std::endl;
            
            // Send a SIGINT signal to our own process to trigger the cleanup handler.
            raise(SIGINT);
            
            // Break the loop and allow the thread to exit.
            break;
        }
        
        // Wait for one second before checking again.
        std::this_thread::sleep_for(std::chrono::seconds(1));
    }
    
    std::cout << "[Monitor] Monitor thread exiting." << std::endl;
}


// ********************************************************************
// *                  MAIN APPLICATION LOGIC                          *
// ********************************************************************

int main(int argc, char* argv[]) {

    if (argc != 2) {
        std::cout << "Usage: " << argv[0] << " <filename>" << std::endl;
        std::cout << "Please provide exactly one filename as an argument." << std::endl;
        return 1;
    }

    std::string filename = argv[1];

    signal(SIGINT, cleanup); // Register signal handler for cleanup
    

    // === Step 1: Create the Shared Memory File IMMEDIATELY ===
    // This allows the main application to detect that the helper has started.
    std::cout << "[Helper] Creating shared memory file..." << std::endl;
    int shm_fd = open(MM_FILE_PATH, O_CREAT | O_RDWR, 0600);
    if (shm_fd == -1) {
        perror("[Helper] CRITICAL: open failed for shared memory file");
        return 1;
    }
    // Set permissions to be world-readable/writable so the Wine process can access it
    if (fchmod(shm_fd, 0666) == -1) {
        perror("[Helper] CRITICAL: fchmod failed on shared memory file");
        close(shm_fd);
        unlink(MM_FILE_PATH);
        return 1;
    }
    if (ftruncate(shm_fd, sizeof(SharedMemory)) == -1) {
        perror("[Helper] CRITICAL: ftruncate failed");
        close(shm_fd);
        unlink(MM_FILE_PATH);
        return 1;
    }

    shared_data = (SharedMemory*)mmap(0, sizeof(SharedMemory), PROT_READ | PROT_WRITE, MAP_SHARED, shm_fd, 0);
    close(shm_fd); // The file descriptor is no longer needed after mmap

    if (shared_data == MAP_FAILED) {
        perror("[Helper] CRITICAL: mmap failed");
        unlink(MM_FILE_PATH); // Use unlink for cleanup
        return 1;
    }

    memset(shared_data, 0, sizeof(SharedMemory));
    std::cout << "[Helper] Memory mapped file created successfully." << std::endl;
    
    std::thread monitor(monitor_parent_process, filename);
    monitor.detach();

    // === Step 2: Perform Interactive Device Detection ===
    std::cout << "[Helper] Starting interactive device detection..." << std::endl;
    std::string keyboard_path;
    std::string mouse_path;
    if (!interactive_device_detection(keyboard_path, mouse_path)) {
        std::cerr << "[Helper] Device detection failed or was cancelled. Exiting." << std::endl;
        cleanup(0); // Clean up the created memory map file
        return 1;
    }
    std::cout << "[Helper] Using Keyboard: " << keyboard_path << std::endl;
    std::cout << "[Helper] Using Mouse:    " << mouse_path << std::endl;

    // === Step 3: Setup Virtual Devices and Worker Threads ===
    std::cout << "[Helper] Starting up core services..." << std::endl;
    uinput_fd = setup_uinput_device();
    if (uinput_fd < 0) {
        cleanup(0);
        return 1;
    }
    std::cout << "[Helper] Virtual input device created." << std::endl;
    
    std::cout << "[Helper] Launching worker threads. Press Ctrl+C to exit." << std::endl;
    std::thread keyboard_reader(evdev_reader_thread, keyboard_path);
    std::thread mouse_reader(evdev_reader_thread, mouse_path);
    std::thread processor(command_processor_thread);
    std::thread special_actions(special_action_thread);
    std::thread fastkey(fastkey_thread);

    keyboard_reader.join();
    mouse_reader.join();
    processor.join();
    special_actions.join();
    fastkey.join();
    
    cleanup(0);
    return 0;
}

void cleanup(int signum) {
    std::cout << "\nCleaning up..." << std::endl;
    if (uinput_fd >= 0) {
        ioctl(uinput_fd, UI_DEV_DESTROY);
        close(uinput_fd);
    }
    if (shared_data != nullptr && shared_data != MAP_FAILED) {
        munmap(shared_data, sizeof(SharedMemory));
    }
    unlink(MM_FILE_PATH);
    exit(0);
}


void fastkey_thread() {
    while (isbhop) {
        bool space_pressed = shared_data->key_states[0x20].load(std::memory_order_acquire);
        if (space_pressed) {
            std::this_thread::sleep_for(std::chrono::milliseconds(bhop_delay/2)); // Short delay

            emit_uinput(EV_KEY, KEY_SPACE, 0);
            emit_uinput(EV_SYN, SYN_REPORT, 0);
            std::this_thread::sleep_for(std::chrono::milliseconds(bhop_delay/2)); // Short delay

            emit_uinput(EV_KEY, KEY_SPACE, 1);
            emit_uinput(EV_SYN, SYN_REPORT, 0);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    while (isdesync) {
        std::this_thread::sleep_for(std::chrono::nanoseconds(100));
        emit_uinput(EV_KEY, desync_itemslot - 1, 1);
        emit_uinput(EV_SYN, SYN_REPORT, 0);
        std::this_thread::sleep_for(std::chrono::nanoseconds(100));
        emit_uinput(EV_KEY, desync_itemslot - 1, 0);
        emit_uinput(EV_SYN, SYN_REPORT, 0);
    }
}

void command_processor_thread() {
    while (true) {
        uint32_t read_idx = shared_data->read_index.load(std::memory_order_acquire);
        uint32_t write_idx = shared_data->write_index.load(std::memory_order_acquire);
        if (read_idx != write_idx) {
            const auto& cmd = shared_data->command_buffer[read_idx];
            CommandType type = cmd.type.load(std::memory_order_relaxed);

            if (type == CMD_KEY_ACTION) {
                uint8_t win_vk = cmd.win_vk_code.load(std::memory_order_relaxed);
                uint16_t evdev_key = win_vkey_to_evdev_key(win_vk);
                if (evdev_key != EVDEV_UNASSIGNED) {
                    printf("[Helper] Emitting key event: VK=0x%02X EVDEV=%d VAL=%d\n", win_vk, evdev_key, cmd.value.load(std::memory_order_relaxed)); // Key event logging
                    emit_uinput(EV_KEY, evdev_key, cmd.value.load(std::memory_order_relaxed));
                }
                emit_uinput(EV_SYN, SYN_REPORT, 0);
            } else if (type == CMD_MOUSE_MOVE_RELATIVE) {
                emit_uinput(EV_REL, REL_X, cmd.rel_x.load(std::memory_order_relaxed));
                emit_uinput(EV_REL, REL_Y, cmd.rel_y.load(std::memory_order_relaxed));
                emit_uinput(EV_SYN, SYN_REPORT, 0);
            } else if (type == CMD_MOUSE_WHEEL) {
                int32_t delta = cmd.value.load(std::memory_order_relaxed);
                printf("[Helper] Emitting mouse wheel event: DELTA=%d\n", delta);
                emit_uinput(EV_REL, REL_WHEEL, delta);  // Emit wheel event (+ = up, - = down)
                emit_uinput(EV_SYN, SYN_REPORT, 0);
            }
            
            else if (type == CMD_SUSPEND_PROCESS) {
                pid_t pid = cmd.target_pid.load(std::memory_order_relaxed);
                printf("[Helper] Received CMD_SUSPEND_PROCESS for PID: %d\n", pid);
                if (pid > 0) {
                    hybrid_suspend_or_resume(pid, true);
                }
            } else if (type == CMD_RESUME_PROCESS) {
                pid_t pid = cmd.target_pid.load(std::memory_order_relaxed);
                printf("[Helper] Received CMD_RESUME_PROCESS for PID: %d\n", pid);
                if (pid > 0) {
                    hybrid_suspend_or_resume(pid, false);
                }
            } else if (type == CMD_BHOP_ENABLE) {
                printf("[Helper] Received CMD_BHOP_ENABLE\n");
                isbhop = true;
                
            } else if (type == CMD_BHOP_DISABLE) {
                printf("[Helper] Received CMD_BHOP_DISABLE\n");
                isbhop = false;
            }
            else if (type == CMD_DESYNC_ENABLE) {
                printf("[Helper] Received CMD_DESYNC_ENABLE\n");
                isdesync = true;
                
            } else if (type == CMD_DESYNC_DISABLE) {
                printf("[Helper] Received CMD_DESYNC_DISABLE\n");
                isdesync = false;
            }

            shared_data->read_index.store((read_idx + 1) & (COMMAND_BUFFER_SIZE - 1), std::memory_order_release);
        } else {
            std::this_thread::sleep_for(std::chrono::microseconds(500));
        }
    }
}

void special_action_thread() {
    while (true) {
        if (shared_data->special_action.request_ready.load(std::memory_order_acquire)) {
            shared_data->special_action.request_ready.store(false, std::memory_order_relaxed);
            SpecialActionCommand cmd = shared_data->special_action.command.load(std::memory_order_relaxed);
            bool success = false;
            int pid_count = 0;
            
            std::string name(shared_data->special_action.process_name);

            if (cmd == SA_FIND_NEWEST_PROCESS) {
                // Calls the new, reliable function to find the main parent process.
                pid_t pid = find_main_process_by_name(name);
                if (pid != -1) {
                    shared_data->special_action.response_pids[0] = pid;
                    pid_count = 1;
                    success = true;
                }
            } else if (cmd == SA_FIND_ALL_PROCESSES) {
                std::vector<pid_t> pids = find_all_processes_by_exes_or_pids(name);
                if (!pids.empty()) {
                    for (size_t i = 0; i < pids.size() && i < 128; ++i) {
                        shared_data->special_action.response_pids[i] = pids[i];
                        pid_count++;
                    }
                    success = true;
                }
            } else if (cmd == SA_SET_BHOP_DELAY) {
                int32_t delay = shared_data->special_action.response_pid_count.load(std::memory_order_relaxed);
                if (delay >= 0) {
                    bhop_delay = delay;
                    success = true;
                    printf("[Helper] Bhop delay set to %d ms\n", bhop_delay);
                }
            } else if (cmd == SA_SET_DESYNC_ITEM) {
                int32_t itemslot = shared_data->special_action.response_pid_count.load(std::memory_order_relaxed);
                if (itemslot >= 1 && itemslot <= 9) {
                    desync_itemslot = itemslot;
                    success = true;
                    printf("[Helper] Desync item slot set to %d\n", desync_itemslot);
                }
            }
            
            shared_data->special_action.response_pid_count.store(pid_count, std::memory_order_relaxed);
            shared_data->special_action.response_success.store(success, std::memory_order_relaxed);
            shared_data->special_action.response_ready.store(true, std::memory_order_release);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
}

// ********************************************************************
// *                  UNCHANGED SUPPORTING FUNCTIONS                  *
// ********************************************************************

std::vector<pid_t> find_all_processes_by_exes_or_pids(const std::string& input) {
    std::vector<pid_t> pids;
    std::vector<std::string> exe_names;
    std::vector<pid_t> pid_tokens;

    // Split input by spaces
    std::istringstream iss(input);
    std::string token;
    while (iss >> token) {
        bool is_pid = true;
        for (char c : token) {
            if (!isdigit(c)) {
                is_pid = false;
                break;
            }
        }

        if (is_pid) {
            try {
                pid_t pid = std::stoi(token);
                if (pid > 0) pid_tokens.push_back(pid);
            } catch (...) {}
        } else {
            exe_names.push_back(token);
        }
    }

    // Add valid PIDs directly
    pids.insert(pids.end(), pid_tokens.begin(), pid_tokens.end());

    // Check if PIDs are valid processes
    pids.erase(std::remove_if(pids.begin(), pids.end(), [](pid_t pid) {
        errno = 0;
        bool invalid = (kill(pid, 0) != 0 && errno != EPERM);

        printf("[Helper] Validating PID: %d - %s\n", pid, invalid ? "Invalid" : "Valid");

        if (!invalid) {
            std::string proc_path = "/proc/" + std::to_string(pid) + "/comm";
            std::ifstream file(proc_path);
            if (file.is_open()) {
                std::string name;
                std::getline(file, name);
                file.close();
                printf("[Helper] PID: %d - Name: %s\n", pid, name.c_str());
            } else {
                printf("[Helper] PID: %d - Unable to read name\n", pid);
            }
        }

        return invalid;
    }), pids.end());


    // If there are no executable names, we’re done
    if (exe_names.empty()) return pids;

    // Scan /proc for processes matching executable names
    DIR* dir = opendir("/proc");
    if (!dir) return pids;

    struct dirent* entry;
    while ((entry = readdir(dir)) != nullptr) {
        pid_t pid = atoi(entry->d_name);
        if (pid <= 0) continue;

        std::string exe_path = "/proc/" + std::string(entry->d_name) + "/exe";
        char link_target[4096] = {0};
        ssize_t len = readlink(exe_path.c_str(), link_target, sizeof(link_target) - 1);
        if (len == -1) continue; // cannot read (permission or gone)
        link_target[len] = '\0';
        std::string target_path(link_target);

        bool matched = false;
        for (const auto& exe : exe_names) {
            if (target_path.size() >= exe.size() &&
                target_path.compare(target_path.size() - exe.size(), exe.size(), exe) == 0) {
                matched = true;
                pids.push_back(pid);
            }
            if (matched) break;
        }
    }

    closedir(dir);
    return pids;
}

/**
 * @brief Replaces the old "newest" process finder with a reliable method.
 *
 * This function finds the "main" process of an application by identifying
 * which of its processes has a parent that is NOT part of the application itself.
 */
pid_t find_main_process_by_name(const std::string& name) {
    std::vector<pid_t> pids = find_all_processes_by_exes_or_pids(name);
    if (pids.empty()) {
        return -1;
    }
    if (pids.size() == 1) {
        return pids[0];
    }

    std::set<pid_t> pid_set(pids.begin(), pids.end());

    for (pid_t pid : pids) {
        pid_t ppid = get_ppid(pid);
        // If the parent of this process is NOT in our set of processes,
        // then this must be the main, top-level process.
        if (pid_set.find(ppid) == pid_set.end()) {
            return pid;
        }
    }

    // Fallback in case of weird process structures. Return the lowest PID.
    return *std::min_element(pids.begin(), pids.end());
}

bool interactive_device_detection(std::string& out_keyboard_path, std::string& out_mouse_path) {
    int epoll_fd = epoll_create1(0);
    if (epoll_fd == -1) { perror("epoll_create1"); return false; }

    std::map<int, std::string> fd_to_path;
    DIR* dir = opendir("/dev/input");
    if (!dir) { perror("opendir /dev/input"); return false; }

    struct dirent* entry;
    while ((entry = readdir(dir)) != nullptr) {
        if (strncmp(entry->d_name, "event", 5) == 0) {
            std::string path = "/dev/input/" + std::string(entry->d_name);
            int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK);
            if (fd < 0) continue;
            struct epoll_event ev_epoll;
            ev_epoll.events = EPOLLIN;
            ev_epoll.data.fd = fd;
            if (epoll_ctl(epoll_fd, EPOLL_CTL_ADD, fd, &ev_epoll) == -1) {
                perror("epoll_ctl");
                close(fd);
                continue;
            }
            fd_to_path[fd] = path;
        }
    }
    closedir(dir);

    bool keyboard_found = false;
    bool mouse_found = false;

    std::cout << "\n>>> Please press any key on your primary keyboard..." << std::endl;

    struct epoll_event events[16];
    while (!keyboard_found || !mouse_found) {
        int n_events = epoll_wait(epoll_fd, events, 16, -1);
        if (n_events == -1) {
            if (errno == EINTR) continue;
            perror("epoll_wait");
            return false;
        }

        for (int i = 0; i < n_events; ++i) {
            struct input_event ev_input;
            while (read(events[i].data.fd, &ev_input, sizeof(ev_input)) > 0) {
                if (!keyboard_found && ev_input.type == EV_KEY && ev_input.value == 1) {
                    if (ev_input.code < BTN_MOUSE || ev_input.code > BTN_JOYSTICK) {
                        out_keyboard_path = fd_to_path[events[i].data.fd];
                        keyboard_found = true;
                        std::cout << ">>> Keyboard found: " << out_keyboard_path << std::endl;
                        if (!mouse_found) std::cout << "\n>>> Please move your primary mouse..." << std::endl;
                    }
                }

                bool is_mouse_movement =
                    (ev_input.type == EV_REL && (ev_input.code == REL_X || ev_input.code == REL_Y)) ||
                    (ev_input.type == EV_ABS && (ev_input.code == ABS_X || ev_input.code == ABS_Y));
                
                if (!mouse_found && is_mouse_movement) {
                    out_mouse_path = fd_to_path[events[i].data.fd];
                    mouse_found = true;
                    std::cout << ">>> Mouse found: " << out_mouse_path << std::endl;
                     if (!keyboard_found) std::cout << "\n>>> Please press any key on your primary keyboard..." << std::endl;
                }
            }
        }
    }

    for (auto const& [fd, path] : fd_to_path) {
        close(fd);
    }
    close(epoll_fd);
    std::cout << "\n>>> Both devices detected. Starting helper..." << std::endl;
    return true;
}

void evdev_reader_thread(const std::string& device_path) {
    int evdev_fd = open(device_path.c_str(), O_RDONLY | O_NONBLOCK);
    if (evdev_fd < 0) {
        perror(("[ReaderThread] Failed to open " + device_path).c_str());
        return;
    }

    ioctl(evdev_fd, EVIOCGRAB, 1); // optional: grab device

    struct input_event ev;
    while (true) {
        ssize_t n = read(evdev_fd, &ev, sizeof(ev));
        if (n != sizeof(ev)) {
            if (n < 0 && errno != EAGAIN && errno != EINTR)
                perror("[ReaderThread] read()");
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
            continue;
        }

        if (ev.type == EV_KEY) {
            uint8_t vk_code = evdev_to_win_vkey(ev.code);
            if (vk_code != VK_UNASSIGNED) {
                if (isbhop && vk_code == 0x20) {
                    // Ignore space key state updates when bhop is enabled
                } else {
                    shared_data->key_states[vk_code].store(ev.value != 0, std::memory_order_release);
                }
            }
            emit_uinput(EV_KEY, ev.code, ev.value);
            emit_uinput(EV_SYN, SYN_REPORT, 0);
        }
        
        else if (ev.type == EV_REL) {
            emit_uinput(EV_REL, ev.code, ev.value);
            emit_uinput(EV_SYN, SYN_REPORT, 0);
        }
        
        else if (ev.type == EV_ABS) {
            emit_uinput(EV_ABS, ev.code, ev.value);
            emit_uinput(EV_SYN, SYN_REPORT, 0);
        }

        
    }

    ioctl(evdev_fd, EVIOCGRAB, 0); // release grab
    close(evdev_fd);
}


int setup_uinput_device() {
    int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
    if (fd < 0) return -1;
    
    ioctl(fd, UI_SET_EVBIT, EV_KEY);
    for (int i = 0; i < KEY_MAX; ++i) ioctl(fd, UI_SET_KEYBIT, i);

    std::cout << "[Helper] Enabling mouse buttons for virtual device." << std::endl;
    ioctl(fd, UI_SET_KEYBIT, BTN_LEFT);
    ioctl(fd, UI_SET_KEYBIT, BTN_RIGHT);
    ioctl(fd, UI_SET_KEYBIT, BTN_MIDDLE);
    ioctl(fd, UI_SET_KEYBIT, BTN_SIDE);
    ioctl(fd, UI_SET_KEYBIT, BTN_EXTRA);
    
    ioctl(fd, UI_SET_EVBIT, EV_REL);
    ioctl(fd, UI_SET_RELBIT, REL_X);
    ioctl(fd, UI_SET_RELBIT, REL_Y);
    ioctl(fd, UI_SET_RELBIT, REL_WHEEL);

    struct uinput_user_dev uidev;
    memset(&uidev, 0, sizeof(uidev));
    snprintf(uidev.name, UINPUT_MAX_NAME_SIZE, "Macro Helper Input");
    uidev.id.bustype = BUS_USB;
    uidev.id.vendor  = 0x1234;
    uidev.id.product = 0xfedc;
    uidev.id.version = 1;

    write(fd, &uidev, sizeof(uidev));
    ioctl(fd, UI_DEV_CREATE);
    sleep(1); 
    return fd;
}

void emit_uinput(int type, int code, int val) {
    struct input_event ie;
    memset(&ie, 0, sizeof(ie));
    ie.type = type;
    ie.code = code;
    ie.value = val;
    write(uinput_fd, &ie, sizeof(ie));
}
