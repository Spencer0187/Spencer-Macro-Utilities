#include "Resource Files/macros.h"
#include <windows.h>
#include <atomic>
#include <vector>
#include <thread>
#include <chrono>

// --- Dependencies from other files ---
#include "Resource Files/keymapping.h"
#include "Resource Files/wine_compatibility_layer.h"

#include "Resource Files/globals.h"

using namespace Globals;

void ItemDesyncLoop()
{
    if (!g_isLinuxWine)
    {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);

        while (running)
        {
            while (running && !isdesyncloop) std::this_thread::sleep_for(std::chrono::milliseconds(1));
            if (!running) break;

            if (macrotoggled && notbinding && section_toggles[1])
            {
                HoldKey(desync_slot + 1);
                ReleaseKey(desync_slot + 1);
                HoldKey(desync_slot + 1);
                ReleaseKey(desync_slot + 1);
            }
        }
    }
    else
    {
        while (running && !isdesyncloop) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running) return;

        SetDesyncState(isdesyncloop);
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
}

void Speedglitchloop()
{
    int sleep1 = 16, sleep2 = 16;
    int last_fps = 0;
    const float EPSILON = 0.008f;

    while (running)
    {
        if (last_fps != RobloxFPS.load(std::memory_order_relaxed))
        {
            float delay_float = 1000.0f / RobloxFPS.load(std::memory_order_relaxed);
            int delay_floor = static_cast<int>(delay_float);
            int delay_ceil = delay_floor + 1;
            float fractional = delay_float - delay_floor;

            if (fractional < 0.33f - EPSILON) sleep1 = sleep2 = delay_floor;
            else if (fractional > 0.66f + EPSILON) sleep1 = sleep2 = delay_ceil;
            else { sleep1 = delay_floor; sleep2 = delay_ceil; }

            last_fps = RobloxFPS.load(std::memory_order_relaxed);
        }

        while (running && !isspeed.load(std::memory_order_relaxed)) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running) break;

        if (macrotoggled && notbinding && section_toggles[3])
        {
            MoveMouse(speed_strengthx, 0);
            std::this_thread::sleep_for(std::chrono::milliseconds(sleep1));
            MoveMouse(speed_strengthy, 0);
            std::this_thread::sleep_for(std::chrono::milliseconds(sleep2));
        }
    }
}


void SpeedglitchloopHHJ()
{
    int sleep1 = 16, sleep2 = 16;
    int last_fps = 0;
    const float EPSILON = 0.008f;

    while (running)
    {
        if (last_fps != RobloxFPS.load(std::memory_order_relaxed))
        {
            float delay_float = 1000.0f / RobloxFPS.load(std::memory_order_relaxed);
            int delay_floor = static_cast<int>(delay_float);
            int delay_ceil = delay_floor + 1;
            float fractional = delay_float - delay_floor;

            if (fractional < 0.33f - EPSILON) sleep1 = sleep2 = delay_floor;
            else if (fractional > 0.66f + EPSILON) sleep1 = sleep2 = delay_ceil;
            else { sleep1 = delay_floor; sleep2 = delay_ceil; }

            last_fps = RobloxFPS.load(std::memory_order_relaxed);
        }

        while (running && !isHHJ.load(std::memory_order_relaxed)) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running) break;

        if (macrotoggled && notbinding && section_toggles[2])
        {
            MoveMouse(speed_strengthx, 0);
            std::this_thread::sleep_for(std::chrono::milliseconds(sleep1));
            MoveMouse(speed_strengthy, 0);
            std::this_thread::sleep_for(std::chrono::milliseconds(sleep2));
        }
    }
}

void SpamKeyLoop(Globals::SpamkeyInstance* inst)
{
    while (running)
    {
        while (running && !inst->should_exit && !inst->thread_active.load(std::memory_order_relaxed)) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running || inst->should_exit) break;

        if (macrotoggled && notbinding && inst->section_enabled)
        {
            HoldKeyBinded(inst->vk_spamkey);
            std::this_thread::sleep_for(std::chrono::milliseconds(inst->real_delay));
            ReleaseKeyBinded(inst->vk_spamkey);
            std::this_thread::sleep_for(std::chrono::milliseconds(inst->real_delay));
        }
    }
}

void ItemClipLoop()
{
    while (running)
    {
        while (running && !isitemloop) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running) break;

        if (macrotoggled && notbinding && section_toggles[8])
        {
            HoldKey(clip_slot + 1);
            std::this_thread::sleep_for(std::chrono::milliseconds(clip_delay / 2));
            ReleaseKey(clip_slot + 1);
            std::this_thread::sleep_for(std::chrono::milliseconds(clip_delay / 2));
        }
    }
}

void WallWalkLoop()
{
    int delay = 16;
    while (running)
    {
        while (running && !iswallwalkloop) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running) break;

        delay = static_cast<unsigned int>(((1000.0f / RobloxFPS.load(std::memory_order_relaxed)) + .5) * 1.1);

        if (macrotoggled && notbinding && section_toggles[10])
        {
            if (wallwalktoggleside)
            {
                MoveMouse(-wallwalk_strengthx, 0);
                std::this_thread::sleep_for(std::chrono::milliseconds(delay));
                MoveMouse(-wallwalk_strengthy, 0);
                std::this_thread::sleep_for(std::chrono::microseconds(RobloxWallWalkValueDelay));
            }
            else
            {
                MoveMouse(wallwalk_strengthx, 0);
                std::this_thread::sleep_for(std::chrono::milliseconds(delay));
                MoveMouse(wallwalk_strengthy, 0);
                std::this_thread::sleep_for(std::chrono::microseconds(RobloxWallWalkValueDelay));
            }
        }
    }
}

void BhopLoop()
{
    if (!g_isLinuxWine)
    {
        while (running)
        {
            while (running && !isbhoploop.load(std::memory_order_acquire)) std::this_thread::sleep_for(std::chrono::milliseconds(1));
            if (!running) break;

            HoldKeyBinded(vk_bunnyhopkey);
            std::this_thread::sleep_for(std::chrono::milliseconds(BunnyHopDelay / 2));
            ReleaseKeyBinded(vk_bunnyhopkey);
            std::this_thread::sleep_for(std::chrono::milliseconds(BunnyHopDelay / 2));
        }
    }
    else
    {
        while (running)
        {
            SetLinuxBhopState(isbhoploop.load(std::memory_order_acquire));
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
    }
}


void WallhopThread(Globals::WallhopInstance* inst)
{
    while (running)
    {
        while (running && !inst->should_exit && !inst->thread_active.load(std::memory_order_acquire)) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running || inst->should_exit) break;

        if (inst->wallhopswitch) MoveMouse(-inst->wallhop_dx, inst->wallhop_vertical);
        else MoveMouse(inst->wallhop_dx, inst->wallhop_vertical);

        if (inst->toggle_flick)
        {
            if (inst->WallhopBonusDelay > 0 && inst->WallhopBonusDelay < inst->WallhopDelay)
            {
                std::this_thread::sleep_for(std::chrono::milliseconds(inst->WallhopBonusDelay));
                if (inst->toggle_jump) HoldKeyBinded(inst->vk_jumpkey);
                std::this_thread::sleep_for(std::chrono::milliseconds(inst->WallhopDelay - inst->WallhopBonusDelay));
            }
            else { if (inst->toggle_jump) HoldKeyBinded(inst->vk_jumpkey); std::this_thread::sleep_for(std::chrono::milliseconds(inst->WallhopDelay)); }

            if (inst->wallhopswitch) MoveMouse(-inst->wallhop_dy, -inst->wallhop_vertical);
            else MoveMouse(inst->wallhop_dy, -inst->wallhop_vertical);
        }
        else { if (inst->toggle_jump) HoldKeyBinded(inst->vk_jumpkey); }

        if (inst->toggle_jump)
        {
            if (100 - inst->WallhopDelay > 0) std::this_thread::sleep_for(std::chrono::milliseconds(100 - inst->WallhopDelay));
            ReleaseKeyBinded(inst->vk_jumpkey);
        }

        inst->thread_active.store(false, std::memory_order_relaxed);
    }
}

void PressKeyThread(Globals::PresskeyInstance* inst)
{
    while (running)
    {
        while (running && !inst->should_exit && !inst->thread_active.load(std::memory_order_relaxed)) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running || inst->should_exit) break;

        if (inst->vk_trigger == inst->vk_presskey) ReleaseKeyBinded(inst->vk_trigger);

        if (inst->PressKeyBonusDelay != 0) std::this_thread::sleep_for(std::chrono::milliseconds(inst->PressKeyBonusDelay));

        HoldKeyBinded(inst->vk_presskey);
        std::this_thread::sleep_for(std::chrono::milliseconds(inst->PressKeyDelay));
        ReleaseKeyBinded(inst->vk_presskey);

        inst->thread_active.store(false, std::memory_order_relaxed);
    }
}

void FloorBounceThread()
{
    while (running)
    {
        while (running && !isfloorbouncethread.load(std::memory_order_relaxed)) std::this_thread::sleep_for(std::chrono::milliseconds(1));
        if (!running) break;

		std::vector<DWORD> targetPIDsBounce = targetPIDs;
		std::vector<HANDLE> hProcessBounce = hProcess;

		HoldKey(0x39); // Hold Space
		std::this_thread::sleep_for(std::chrono::milliseconds(521));			  // Fall timing
		SuspendOrResumeProcesses_Compat(targetPIDsBounce, hProcessBounce, true);  // Freeze to clip through floor
		std::this_thread::sleep_for(std::chrono::milliseconds(72));				  // Stay clipped
		SuspendOrResumeProcesses_Compat(targetPIDsBounce, hProcessBounce, false); // Register underground position
		std::this_thread::sleep_for(std::chrono::milliseconds(72));				  // Let correction force build
		SuspendOrResumeProcesses_Compat(targetPIDsBounce, hProcessBounce, true);  // Freeze the ejection force
		std::this_thread::sleep_for(std::chrono::milliseconds(72));				  // Hold the power
		SuspendOrResumeProcesses_Compat(targetPIDsBounce, hProcessBounce, false); // LAUNCH
		ReleaseKey(0x39);														  // Release Space

		if (floorbouncehhj) {
			// Completely guessed values
			std::this_thread::sleep_for(std::chrono::milliseconds(FloorBounceDelay1)); // Delay after unfreezing and before shiftlocking
			HoldKeyBinded(vk_shiftkey);
			std::this_thread::sleep_for(std::chrono::milliseconds(FloorBounceDelay2)); // Delay before enabling helicoptering
			isHHJ.store(true, std::memory_order_relaxed);
			std::this_thread::sleep_for(std::chrono::milliseconds(FloorBounceDelay3)); // Time spent helicoptering
			isHHJ.store(false, std::memory_order_relaxed);
			ReleaseKeyBinded(vk_shiftkey);
		}

        isfloorbouncethread = false;
    }
}

void PasteText(const std::string &text)
{
	if (g_isLinuxWine) {
		// Always use compat version for Linux/Wine
		for (char c : text) {
			KeyAction action = CharToKeyAction_Compat(c);

			if (action.vk_code == 0)
				continue; // Skip unmappable characters

			if (action.needs_shift) {
				HoldKeyBinded(VK_SHIFT);
			}

			HoldKeyBinded(action.vk_code);
			std::this_thread::sleep_for(std::chrono::milliseconds(PasteDelay));
			ReleaseKeyBinded(action.vk_code);

			if (action.needs_shift) {
				ReleaseKeyBinded(VK_SHIFT);
			}
		}
	} else if (useoldpaste) {
		// Windows with old paste: Use Unicode method
		INPUT input = {0};
		input.type = INPUT_KEYBOARD;
		input.ki.wVk = 0;
		input.ki.dwFlags = KEYEVENTF_UNICODE;

		for (char c : text) {
			// Windows can handle many characters directly with wScan
			input.ki.wScan = c;
			input.ki.dwFlags = KEYEVENTF_UNICODE;
			SendInput(1, &input, sizeof(INPUT));
			input.ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;
			SendInput(1, &input, sizeof(INPUT));
			std::this_thread::sleep_for(std::chrono::milliseconds(PasteDelay));
		}
	} else {
		// Windows with new paste: Use global scan code method
		for (char c : text) {
			KeyAction action = CharToKeyAction_Global(c);

			if (action.vk_code == 0)
				continue; // Skip unmappable characters

			if (action.needs_shift) {
				HoldKeyBinded(VK_SHIFT); // Keep using VK_SHIFT for shift
			}

			bool extended = (action.scan_code & 0xE000) != 0;
			WORD base_scan_code = action.scan_code & 0x00FF; // Remove extended flag

			HoldKey(base_scan_code, extended);
			std::this_thread::sleep_for(std::chrono::milliseconds(PasteDelay));
			ReleaseKey(base_scan_code, extended);

			if (action.needs_shift) {
				ReleaseKeyBinded(VK_SHIFT);
			}
		}
	}
}