#ifndef CRONET_RS_NAIVE_H_
#define CRONET_RS_NAIVE_H_

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef int (*Cronet_DialerFunc)(
    void* context, char* address, uint16_t port);
typedef int (*Cronet_UdpDialerFunc)(
    void* context, char* address, uint16_t port,
    char* out_local_address, uint16_t* out_local_port);

stream_engine* Cronet_Engine_GetStreamEngine(Cronet_EnginePtr);
void Cronet_Engine_SetDialer(Cronet_EnginePtr, Cronet_DialerFunc, void*);
void Cronet_Engine_SetUdpDialer(Cronet_EnginePtr, Cronet_UdpDialerFunc, void*);
void Cronet_Engine_CloseAllConnections(Cronet_EnginePtr);
void* Cronet_CreateCertVerifierWithRootCerts(Cronet_String);
void Cronet_Engine_SetMockCertVerifierForTesting(Cronet_EnginePtr, void*);

#ifdef __cplusplus
}
#endif
#endif

