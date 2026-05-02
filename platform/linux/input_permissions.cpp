#include "input_permissions.h"

#include <algorithm>
#include <cerrno>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <string>
#include <sys/stat.h>
#include <unistd.h>
#include <vector>

namespace smu::platform::linux {
namespace {

std::string ErrnoText(const std::string& prefix)
{
    return prefix + ": " + std::strerror(errno);
}

std::vector<std::string> EnumerateEventDevices()
{
    std::vector<std::string> devices;
    DIR* inputDir = opendir("/dev/input");
    if (!inputDir) {
        return devices;
    }

    while (dirent* entry = readdir(inputDir)) {
        if (std::strncmp(entry->d_name, "event", 5) == 0) {
            devices.push_back("/dev/input/" + std::string(entry->d_name));
        }
    }

    closedir(inputDir);
    std::sort(devices.begin(), devices.end());
    return devices;
}

bool CanReadFromEventDevice(int fd)
{
    input_event event {};
    for (;;) {
        errno = 0;
        const ssize_t bytesRead = read(fd, &event, sizeof(event));
        if (bytesRead == sizeof(event)) {
            return true;
        }
        if (bytesRead < 0 && errno == EINTR) {
            continue;
        }
        if (bytesRead < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
            return true;
        }
        return false;
    }
}

} // namespace

bool CanReadInputEvents()
{
    return GetInputPermissionStatus().canReadInputEvents;
}

bool CanOpenUinput()
{
    return GetInputPermissionStatus().canOpenUinput;
}

InputPermissionStatus GetInputPermissionStatus()
{
    InputPermissionStatus status;

    struct stat inputStat {};
    if (stat("/dev/input", &inputStat) != 0) {
        status.inputEventsMessage = ErrnoText("Cannot access /dev/input");
    } else if (!S_ISDIR(inputStat.st_mode)) {
        status.inputEventsMessage = "/dev/input exists but is not a directory.";
    } else {
        status.inputDirectoryAvailable = true;
        const std::vector<std::string> devices = EnumerateEventDevices();
        if (devices.empty()) {
            status.inputEventsMessage = "No /dev/input/event* devices were found.";
        }

        std::string lastOpenError;
        for (const std::string& path : devices) {
            errno = 0;
            const int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
            if (fd < 0) {
                lastOpenError = ErrnoText("Cannot open " + path);
                continue;
            }

            if (CanReadFromEventDevice(fd)) {
                ++status.readableEventDeviceCount;
            } else {
                lastOpenError = ErrnoText("Cannot read " + path);
            }
            close(fd);
        }

        status.canReadInputEvents = status.readableEventDeviceCount > 0;
        if (status.canReadInputEvents) {
            status.inputEventsMessage = "Readable /dev/input/event* devices: " + std::to_string(status.readableEventDeviceCount);
        } else if (status.inputEventsMessage.empty()) {
            status.inputEventsMessage = lastOpenError.empty()
                ? "No readable /dev/input/event* devices were found."
                : lastOpenError;
        }
    }

    errno = 0;
    const int uinputFd = open("/dev/uinput", O_WRONLY | O_NONBLOCK | O_CLOEXEC);
    if (uinputFd < 0) {
        status.uinputMessage = ErrnoText("Cannot open /dev/uinput for writing");
    } else {
        errno = 0;
        const ssize_t bytesWritten = write(uinputFd, "", 0);
        status.canOpenUinput = bytesWritten == 0;
        if (status.canOpenUinput) {
            status.uinputMessage = "/dev/uinput is writable.";
        } else {
            status.uinputMessage = ErrnoText("Cannot write to /dev/uinput");
        }
        close(uinputFd);
    }

    return status;
}

} // namespace smu::platform::linux
