#pragma once

#if defined(_WIN32)

namespace smu::platform::windows {

bool IsRunAsAdmin();
bool RestartAsAdmin();

} // namespace smu::platform::windows

#endif
