#ifndef SHARED_MEM_H
#define SHARED_MEM_H

#include <atomic>
#include <cstdint>

#define CACHE_LINE_SIZE 64

// --- High-Speed Commands ---
enum CommandType {
	CMD_NONE = 0,
	CMD_KEY_ACTION,
	CMD_MOUSE_MOVE_RELATIVE,
	CMD_MOUSE_WHEEL,
	CMD_SUSPEND_PROCESS,
	CMD_RESUME_PROCESS,
	CMD_BHOP_ENABLE,
	CMD_BHOP_DISABLE
};
const uint32_t COMMAND_BUFFER_SIZE = 256;
struct Command {
	std::atomic<CommandType> type;
	std::atomic<uint8_t> win_vk_code;
	std::atomic<int32_t> value;
	std::atomic<int32_t> rel_x;
	std::atomic<int32_t> rel_y;
	std::atomic<int32_t> target_pid;
};

// --- Low-Speed Mailbox ---
enum SpecialActionCommand {
	SA_NONE = 0,
	SA_FIND_NEWEST_PROCESS,
	SA_FIND_ALL_PROCESSES, // For the "takeallprocessids" feature
};

struct SpecialAction {
	std::atomic<bool> request_ready;
	std::atomic<bool> response_ready;
	std::atomic<SpecialActionCommand> command;
	char process_name[256];
	std::atomic<bool> response_success;

	// Response can now hold a list of PIDs
	std::atomic<int32_t> response_pid_count;
	int32_t response_pids[128];
};

// --- Main Structure ---
struct alignas(CACHE_LINE_SIZE) SharedMemory {
	std::atomic<bool> key_states[256];
	alignas(CACHE_LINE_SIZE) std::atomic<uint32_t> write_index;
	alignas(CACHE_LINE_SIZE) std::atomic<uint32_t> read_index;
	Command command_buffer[COMMAND_BUFFER_SIZE];
	alignas(CACHE_LINE_SIZE) SpecialAction special_action;
};

#endif