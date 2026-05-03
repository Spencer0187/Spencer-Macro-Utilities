#include "windows_backends.h"

#include "input_sendinput.h"
#include "network_windivert.h"
#include "process_win32.h"

#include "../input_backend.h"
#include "../network_backend.h"
#include "../process_backend.h"

namespace smu::platform::windows {

void InitializeWindowsPlatformBackends()
{
    smu::platform::SetInputBackend(smu::platform::windows::CreateWindowsInputBackend());
    smu::platform::SetProcessBackend(smu::platform::windows::CreateWindowsProcessBackend());
    smu::platform::SetNetworkLagBackend(smu::platform::windows::CreateWinDivertNetworkLagBackend());
}

} // namespace smu::platform::windows
