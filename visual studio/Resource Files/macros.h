#pragma once
#include <string>

namespace Globals {
    struct WallhopInstance;
    struct PresskeyInstance;
    struct SpamkeyInstance;
}

// --- Macro Thread Functions ---
void ItemDesyncLoop();
void Speedglitchloop();
void SpeedglitchloopHHJ();
void SpamKeyLoop(Globals::SpamkeyInstance* inst);
void ItemClipLoop();
void WallWalkLoop();
void BhopLoop();
void WallhopThread(Globals::WallhopInstance* inst);
void PressKeyThread(Globals::PresskeyInstance* inst);
void FloorBounceThread();

// --- Helper Functions ---
void PasteText(const std::string &text);