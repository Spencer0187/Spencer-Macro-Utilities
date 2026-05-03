#include "file_dialog.h"

namespace smu::platform {

#if !defined(_WIN32) && !defined(__linux__)
FileDialogResult OpenNativeFileDialog(const FileDialogOptions&)
{
    return {FileDialogResultType::Unavailable, {}, "No native file dialog is available for this platform."};
}
#endif

} // namespace smu::platform
