#pragma once

namespace smu::app {

void DrawKeyBindControlShared(const char* id,
    unsigned int& key,
    int currentSection,
    float humanWidth = 170.0f,
    float hexWidth = 130.0f,
    bool wrapKeyBindingLabel = false);

} // namespace smu::app
