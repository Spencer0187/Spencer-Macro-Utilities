#ifndef ROLOGPARSER_H
#define ROLOGPARSER_H
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#define MAX_LINE_BUFFER_LENGHT 4096

enum RoLogServerType {
    ROLOGINVALIDSERVER,
    ROLOGRCCSERVER,
    ROLOGUDMUXSERVER
};

typedef struct RoLogIP {
    char* IP;
    uint16_t Port;
} RoLogIP;

typedef struct RoLogServerIPs {
    enum RoLogServerType serverType;
    char* serverIPRCC;
    uint16_t serverPortRCC;
    char* serverIPUDMUX;
    uint16_t serverPortUDMUX;
} RoLogServerIPs;

typedef struct RoLogObject {
    unsigned int last_idx;
    const char* file_path;
    char current_state;
    uint64_t placeId;
    uint64_t userId;
    uint64_t universeId;
    RoLogServerIPs* serverObj;
    RoLogIP* playerIP;
    char* serverRobloxGitHash;
    char* serverPrefix;
    char* serverLuauVersion;
    char* serverJobId;
    char* clientRobloxGitHash;
    char* clientChannel;
    uint16_t resolutionWidth;
    uint16_t resolutionHeight;
} RoLogObject;

enum RoLogParseRes {
    ROLOGSUCCESS,
    ROLOGINVALID_FILE_PATH,
};

enum RoLogState {
    ROLOG_IN_LUA_APP,
    ROLOG_IN_GAME,
    ROLOG_OFFLINE,
    ROLOG_INVALID
};

static inline RoLogObject* RoLogCreateObject(const char* file_path) {
    RoLogObject* obj = (RoLogObject *)malloc(sizeof(RoLogObject));
    if (file_path != NULL) {
        obj->file_path = file_path;
    }
    obj->last_idx = 0;
    obj->current_state = 3;
    obj->placeId = 0;
    obj->userId = 0;
    obj->universeId = 0;

    RoLogServerIPs* serverObj = (RoLogServerIPs *)malloc(sizeof(RoLogServerIPs));
    serverObj->serverIPRCC = (char*)malloc(64);
    serverObj->serverPortRCC = 0;
    serverObj->serverIPUDMUX = (char*)malloc(64);
    serverObj->serverPortUDMUX = 0;
    serverObj->serverType = ROLOGINVALIDSERVER;

    obj->serverObj = serverObj;
    obj->playerIP = NULL;
    obj->serverRobloxGitHash = NULL;
    obj->serverPrefix = NULL;
    obj->serverLuauVersion = NULL;
    obj->serverJobId = NULL;
    obj->clientRobloxGitHash = NULL;
    obj->clientChannel = NULL;
    obj->resolutionHeight = 0;
    obj->resolutionWidth = 0;
    return obj;
}

static inline void RoLogFreeObject(RoLogObject* obj) {
    if (!obj) return;
    if (obj->serverObj) {
        free(obj->serverObj->serverIPRCC);
        free(obj->serverObj->serverIPUDMUX);
        free(obj->serverObj);
    }
    if (obj->playerIP) {
        free(obj->playerIP->IP);
        free(obj->playerIP);
    }
    free(obj->serverRobloxGitHash);
    free(obj->serverPrefix);
    free(obj->serverLuauVersion);
    free(obj->serverJobId);
    free(obj->clientRobloxGitHash);
    free(obj->clientChannel);
    free(obj);
}

static inline uint64_t RoLogFindCommaValue(char* line, const char* regex) {
    char* regexLine = strstr(line, regex);
    if (regexLine) {
        const char *regexIndex = strchr(regexLine, ':');
        if (regexIndex) {
            regexIndex++; // :
            const char *regexEnd = strchr(regexIndex, ',');

            size_t len = regexEnd - regexIndex;
            char value[64];

            if (len >= sizeof(value)) {len = sizeof(value) - 1;}
            memcpy(value, regexIndex, len);
            value[len] = '\0';
            return strtoull(value, NULL, 10);
        }
    }
    return 0;
}

static inline void RoLogDealWithGameJoinLoadTimeValues(RoLogObject* obj, char* line) {
    uint64_t placeId = RoLogFindCommaValue(line, "placeid:");
    if (placeId) {
        obj->placeId = placeId;
    }

    uint64_t userId = RoLogFindCommaValue(line, "userid:");
    if (userId) {
        obj->userId = userId;
    }

    uint64_t universeId = RoLogFindCommaValue(line, "universeid:");
    if (universeId) {
        obj->universeId = universeId;
    }
}

static inline RoLogIP* RoLogExtractIP(char *line, int idx) {
    RoLogIP *IPobj = (RoLogIP*)malloc(sizeof *IPobj);
    if (!IPobj)
        return NULL;

    char *p = line + idx;
    char *colon = strchr(p, ':');

    if (!colon) {
        free(IPobj);
        return NULL;
    }

    size_t ip_len = colon - p;

    IPobj->IP = (char*)malloc(ip_len + 1);
    if (!IPobj->IP) {
        free(IPobj);
        return NULL;
    }

    memcpy(IPobj->IP, p, ip_len);
    IPobj->IP[ip_len] = '\0';

    IPobj->Port = (uint16_t)strtoul(colon + 1, NULL, 10);

    return IPobj;
}

static inline void RoLogDealWithConnectingTo(RoLogObject* obj, char* line) {
    if (obj->serverObj->serverType == ROLOGINVALIDSERVER || obj->serverObj->serverIPRCC == NULL || obj->serverObj->serverPortRCC == 0) {
        char *a = strstr(line, "Connecting to ");
        if (!a) return;

        if (strstr(line, "UDMUX") && strstr(line, "RCC")) {
            obj->serverObj->serverType = ROLOGUDMUXSERVER;
            const char *a = strstr(line, "UDMUX server ");
            if (!a) return;

            a += strlen("UDMUX server ");

            RoLogIP *UdmuxIP = RoLogExtractIP(line, a - line);
            if (!UdmuxIP) return;

            strcpy(obj->serverObj->serverIPUDMUX, UdmuxIP->IP);
            obj->serverObj->serverPortUDMUX = UdmuxIP->Port;

            free(UdmuxIP->IP);
            free(UdmuxIP);

            const char *rcc = strstr(line, "RCC server ");
            if (!rcc) return;
            rcc += strlen("RCC server ");

            RoLogIP* RccIP = RoLogExtractIP(line, rcc - line);
            if (!RccIP) return;

            strcpy(obj->serverObj->serverIPRCC, RccIP->IP);
            obj->serverObj->serverPortRCC = RccIP->Port;

            free(RccIP->IP);
            free(RccIP);
        } else {
            obj->serverObj->serverType = ROLOGRCCSERVER;
            const char* a = strstr(line, "Connecting to ");
            if (!a) return;
            a += strlen("Connecting to ");

            RoLogIP *ip = RoLogExtractIP(line, a - line);
            if (!ip) return;

            strcpy(obj->serverObj->serverIPRCC, ip->IP);
            obj->serverObj->serverPortRCC = ip->Port;

            free(ip->IP);
            free(ip);
        }
    } 
}

static inline void RoLogDealWithUDMUXAddress(RoLogObject* obj, char* line) {
    if (obj->serverObj->serverType == ROLOGINVALIDSERVER || obj->serverObj->serverIPRCC == NULL || obj->serverObj->serverPortRCC == 0) {
        const char* udmux = strstr(line, "UDMUX Address = ");
        if (!udmux) return;
        udmux += strlen("UDMUX Address = ");

        const char* udmux_comma = strchr(udmux, ',');
        if (!udmux_comma) return;

        size_t udmux_len = udmux_comma - udmux;
        memcpy(obj->serverObj->serverIPUDMUX, udmux, udmux_len);
        obj->serverObj->serverIPUDMUX[udmux_len] = '\0';

        const char* udmux_port = strstr(udmux_comma, "Port = ");
        if (!udmux_port) return;
        udmux_port += strlen("Port = ");
        obj->serverObj->serverPortUDMUX = (uint16_t)strtoul(udmux_port, NULL, 10);

        const char* rcc = strstr(line, "RCC Server Address = ");
        if (!rcc) return;
        rcc += strlen("RCC Server Address = ");

        const char* rcc_comma = strchr(rcc, ',');
        if (!rcc_comma) return;

        size_t rcc_len = rcc_comma - rcc;
        memcpy(obj->serverObj->serverIPRCC, rcc, rcc_len);
        obj->serverObj->serverIPRCC[rcc_len] = '\0';

        const char* rcc_port = strstr(rcc_comma, "Port = ");
        if (!rcc_port) return;
        rcc_port += strlen("Port = ");
        obj->serverObj->serverPortRCC = (uint16_t)strtoul(rcc_port, NULL, 10);

        obj->serverObj->serverType = ROLOGUDMUXSERVER;
    }
}

static inline char* RoLogExtractAfterColon(const char* line, const char* label) {
    const char* found = strstr(line, label);
    if (!found) return NULL;

    const char* colon = strchr(found, ':');
    if (!colon) return NULL;

    colon++;
    if (*colon == ' ') colon++;

    size_t len = strlen(colon);

    while (len > 0 && (colon[len-1] == '\n' || colon[len-1] == '\r')) {
        len--;
    } 

    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    memcpy(result, colon, len);
    result[len] = '\0';
    return result;
}

static inline char* RoLogExtractLuauVersionFromGitHash(char* gitHash) {
    if (!gitHash) return NULL;

    const char *start = gitHash;
    const char *first = strchr(start, '.');
    if (!first) return NULL;

    const char *second = strchr(first + 1, '.');
    if (!second) return NULL;

    size_t len = second - start;

    char *out = (char*)malloc(len + 1);
    if (!out) return NULL;

    memcpy(out, start, len);
    out[len] = '\0';

    return out;
}

static inline void RoLogDealWithJoiningGame(RoLogObject* obj, char* line) {
    const char* marker = strstr(line, "Joining game '");
    if (!marker) return;
    marker += strlen("Joining game '");

    const char* end = strchr(marker, '\'');
    if (!end) return;

    size_t len = end - marker;
    free(obj->serverJobId);
    obj->serverJobId = (char*)malloc(len + 1);
    if (!obj->serverJobId) return;
    memcpy(obj->serverJobId, marker, len);
    obj->serverJobId[len] = '\0';
}

static inline void RoLogDealWithChannel(RoLogObject* obj, char* line) {
    const char* marker = strstr(line, "The channel is ");
    if (!marker) return;
    marker += strlen("The channel is ");

    size_t len = strlen(marker);
    while (len > 0 && (marker[len-1] == '\n' || marker[len-1] == '\r')) {len--;}

    free(obj->clientChannel);
    obj->clientChannel = (char*)malloc(len + 1);
    if (!obj->clientChannel) return;
    memcpy(obj->clientChannel, marker, len);
    obj->clientChannel[len] = '\0';
}

static inline void RoLogDealWithResolution(RoLogObject* obj, char* line) {
    const char* marker = strstr(line, "resizing main targets to ");
    if (!marker) return;
    marker += strlen("resizing main targets to ");

    const char* x = strchr(marker, 'x');
    if (!x) return;

    size_t w_len = x - marker;
    char width[16];
    if (w_len >= sizeof(width)) return;
    memcpy(width, marker, w_len);
    width[w_len] = '\0';

    char height[16];
    size_t h_len = strlen(x + 1);
    while (h_len > 0 && (x[1 + h_len - 1] == '\n' || x[1 + h_len - 1] == '\r'))
        h_len--;
    if (h_len >= sizeof(height)) return;
    memcpy(height, x + 1, h_len);
    height[h_len] = '\0';

    obj->resolutionWidth = (uint16_t)strtoul(width, NULL, 10);
    obj->resolutionHeight = (uint16_t)strtoul(height, NULL, 10);
}

static inline enum RoLogParseRes RoLogParse(RoLogObject* obj){
    if (obj->file_path == NULL) return ROLOGINVALID_FILE_PATH;
    FILE* fp = fopen(obj->file_path, "r");
    if (!fp) {
        return ROLOGINVALID_FILE_PATH;
    }

    fseek(fp, obj->last_idx, SEEK_SET);

    char line[MAX_LINE_BUFFER_LENGHT];
    while(fgets(line, sizeof(line), fp)) {
        if (strstr(line, "returnToLuaApp")) {
            obj->current_state = ROLOG_IN_LUA_APP;
        }
        if (strstr(line, "setStage: (stage:None)")) {
            obj->current_state = ROLOG_OFFLINE;
        }
        if (strstr(line, "Report game_join_loadtime")) {
            obj->current_state = ROLOG_IN_GAME;
            RoLogDealWithGameJoinLoadTimeValues(obj, line);
        }
        if (strstr(line, "Joining game '")) {
            RoLogDealWithJoiningGame(obj, line);
        }
        if (strstr(line, "Connecting to")) {
            RoLogDealWithConnectingTo(obj, line);
        }
        // Alternative way to get server addresses.
        if (strstr(line, "UDMUX Address")) {
            RoLogDealWithUDMUXAddress(obj, line);
        }
        if (strstr(line, "Server RobloxGitHash")) {
            free(obj->serverRobloxGitHash);
            obj->serverRobloxGitHash = RoLogExtractAfterColon(line, "Server RobloxGitHash");
        }
        if (strstr(line, "Server Prefix")) {
            free(obj->serverPrefix);
            obj->serverPrefix = RoLogExtractAfterColon(line, "Server Prefix");
            free(obj->serverLuauVersion);
            obj->serverLuauVersion = RoLogExtractLuauVersionFromGitHash(obj->serverPrefix);
        }
        if (strstr(line, "[FLog::ClientRunInfo] RobloxGitHash")) {
            free(obj->clientRobloxGitHash);
            obj->clientRobloxGitHash = RoLogExtractAfterColon(line, "RobloxGitHash");
        }
        if (strstr(line, "[FLog::ClientRunInfo] The channel is")) {
            RoLogDealWithChannel(obj, line);
        }
        if (strstr(line, "resizing main targets to")) {
            RoLogDealWithResolution(obj, line);
        }
    }
    obj->last_idx = (unsigned int)ftell(fp);
    fclose(fp);

    return ROLOGSUCCESS;
}

#endif
