#ifndef CRONET_RS_DEV_H_
#define CRONET_RS_DEV_H_

/*
 * Development-only declarations for building docs and the safe wrapper
 * without a Chromium checkout. Production bindings are generated from the
 * SDK's complete cronet.idl_c.h.
 */
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef const char* Cronet_String;
typedef void* Cronet_RawDataPtr;
typedef void* Cronet_ClientContext;
typedef struct Cronet_Engine* Cronet_EnginePtr;
typedef struct Cronet_EngineParams* Cronet_EngineParamsPtr;
typedef struct Cronet_UrlRequest* Cronet_UrlRequestPtr;
typedef struct Cronet_UrlRequestParams* Cronet_UrlRequestParamsPtr;
typedef struct Cronet_UrlRequestCallback* Cronet_UrlRequestCallbackPtr;
typedef struct Cronet_Executor* Cronet_ExecutorPtr;
typedef struct Cronet_Buffer* Cronet_BufferPtr;
typedef struct Cronet_UrlRequestStatusListener* Cronet_UrlRequestStatusListenerPtr;
typedef struct Cronet_HttpHeader* Cronet_HttpHeaderPtr;
typedef struct Cronet_UploadDataProvider* Cronet_UploadDataProviderPtr;
typedef struct Cronet_UploadDataSink* Cronet_UploadDataSinkPtr;
typedef struct Cronet_Runnable* Cronet_RunnablePtr;
typedef struct Cronet_UrlResponseInfo* Cronet_UrlResponseInfoPtr;
typedef struct Cronet_Error* Cronet_ErrorPtr;
typedef struct Cronet_QuicHint* Cronet_QuicHintPtr;
typedef struct Cronet_PublicKeyPins* Cronet_PublicKeyPinsPtr;

typedef enum Cronet_RESULT {
  Cronet_RESULT_SUCCESS = 0,
  Cronet_RESULT_ILLEGAL_ARGUMENT = -100,
  Cronet_RESULT_ILLEGAL_ARGUMENT_STORAGE_PATH_MUST_EXIST = -101,
  Cronet_RESULT_ILLEGAL_ARGUMENT_INVALID_PIN = -102,
  Cronet_RESULT_ILLEGAL_ARGUMENT_INVALID_HOSTNAME = -103,
  Cronet_RESULT_ILLEGAL_ARGUMENT_INVALID_HTTP_METHOD = -104,
  Cronet_RESULT_ILLEGAL_ARGUMENT_INVALID_HTTP_HEADER = -105,
  Cronet_RESULT_ILLEGAL_STATE = -200,
  Cronet_RESULT_ILLEGAL_STATE_STORAGE_PATH_IN_USE = -201,
  Cronet_RESULT_ILLEGAL_STATE_CANNOT_SHUTDOWN_ENGINE_FROM_NETWORK_THREAD = -202,
  Cronet_RESULT_ILLEGAL_STATE_ENGINE_ALREADY_STARTED = -203,
  Cronet_RESULT_ILLEGAL_STATE_REQUEST_ALREADY_STARTED = -204,
  Cronet_RESULT_ILLEGAL_STATE_REQUEST_NOT_INITIALIZED = -205,
  Cronet_RESULT_ILLEGAL_STATE_REQUEST_ALREADY_INITIALIZED = -206,
  Cronet_RESULT_ILLEGAL_STATE_REQUEST_NOT_STARTED = -207,
  Cronet_RESULT_NULL_POINTER = -300,
  Cronet_RESULT_NULL_POINTER_HOSTNAME = -301,
  Cronet_RESULT_NULL_POINTER_SHA256_PINS = -302,
  Cronet_RESULT_NULL_POINTER_EXPIRATION_DATE = -303,
  Cronet_RESULT_NULL_POINTER_ENGINE = -304,
  Cronet_RESULT_NULL_POINTER_URL = -305,
  Cronet_RESULT_NULL_POINTER_CALLBACK = -306,
  Cronet_RESULT_NULL_POINTER_EXECUTOR = -307
} Cronet_RESULT;

typedef enum Cronet_EngineParams_HTTP_CACHE_MODE {
  Cronet_EngineParams_HTTP_CACHE_MODE_DISABLED = 0,
  Cronet_EngineParams_HTTP_CACHE_MODE_IN_MEMORY = 1,
  Cronet_EngineParams_HTTP_CACHE_MODE_DISK_NO_HTTP = 2,
  Cronet_EngineParams_HTTP_CACHE_MODE_DISK = 3
} Cronet_EngineParams_HTTP_CACHE_MODE;

typedef enum Cronet_UrlRequestParams_REQUEST_PRIORITY {
  Cronet_UrlRequestParams_REQUEST_PRIORITY_IDLE = 0,
  Cronet_UrlRequestParams_REQUEST_PRIORITY_LOWEST = 1,
  Cronet_UrlRequestParams_REQUEST_PRIORITY_LOW = 2,
  Cronet_UrlRequestParams_REQUEST_PRIORITY_MEDIUM = 3,
  Cronet_UrlRequestParams_REQUEST_PRIORITY_HIGHEST = 4
} Cronet_UrlRequestParams_REQUEST_PRIORITY;

typedef enum Cronet_Error_ERROR_CODE {
  Cronet_Error_ERROR_CODE_ERROR_CALLBACK = 0,
  Cronet_Error_ERROR_CODE_ERROR_HOSTNAME_NOT_RESOLVED = 1,
  Cronet_Error_ERROR_CODE_ERROR_INTERNET_DISCONNECTED = 2,
  Cronet_Error_ERROR_CODE_ERROR_NETWORK_CHANGED = 3,
  Cronet_Error_ERROR_CODE_ERROR_TIMED_OUT = 4,
  Cronet_Error_ERROR_CODE_ERROR_CONNECTION_CLOSED = 5,
  Cronet_Error_ERROR_CODE_ERROR_CONNECTION_TIMED_OUT = 6,
  Cronet_Error_ERROR_CODE_ERROR_CONNECTION_REFUSED = 7,
  Cronet_Error_ERROR_CODE_ERROR_CONNECTION_RESET = 8,
  Cronet_Error_ERROR_CODE_ERROR_ADDRESS_UNREACHABLE = 9,
  Cronet_Error_ERROR_CODE_ERROR_QUIC_PROTOCOL_FAILED = 10,
  Cronet_Error_ERROR_CODE_ERROR_OTHER = 11
} Cronet_Error_ERROR_CODE;

typedef int32_t Cronet_UrlRequestStatusListener_Status;
typedef void (*Cronet_UrlRequestStatusListener_OnStatusFunc)(
    Cronet_UrlRequestStatusListenerPtr, Cronet_UrlRequestStatusListener_Status);
Cronet_UrlRequestStatusListenerPtr Cronet_UrlRequestStatusListener_CreateWith(
    Cronet_UrlRequestStatusListener_OnStatusFunc);
void Cronet_UrlRequestStatusListener_Destroy(Cronet_UrlRequestStatusListenerPtr);
void Cronet_UrlRequestStatusListener_SetClientContext(
    Cronet_UrlRequestStatusListenerPtr, Cronet_ClientContext);
Cronet_ClientContext Cronet_UrlRequestStatusListener_GetClientContext(
    Cronet_UrlRequestStatusListenerPtr);

Cronet_EnginePtr Cronet_Engine_Create(void);
void Cronet_Engine_Destroy(Cronet_EnginePtr);
Cronet_RESULT Cronet_Engine_StartWithParams(Cronet_EnginePtr, Cronet_EngineParamsPtr);
Cronet_RESULT Cronet_Engine_Shutdown(Cronet_EnginePtr);
bool Cronet_Engine_StartNetLogToFile(Cronet_EnginePtr, Cronet_String, bool);
void Cronet_Engine_StopNetLog(Cronet_EnginePtr);
Cronet_String Cronet_Engine_GetVersionString(Cronet_EnginePtr);
Cronet_String Cronet_Engine_GetDefaultUserAgent(Cronet_EnginePtr);

Cronet_EngineParamsPtr Cronet_EngineParams_Create(void);
void Cronet_EngineParams_Destroy(Cronet_EngineParamsPtr);
void Cronet_EngineParams_user_agent_set(Cronet_EngineParamsPtr, Cronet_String);
void Cronet_EngineParams_accept_language_set(Cronet_EngineParamsPtr, Cronet_String);
void Cronet_EngineParams_storage_path_set(Cronet_EngineParamsPtr, Cronet_String);
void Cronet_EngineParams_enable_quic_set(Cronet_EngineParamsPtr, bool);
void Cronet_EngineParams_enable_http2_set(Cronet_EngineParamsPtr, bool);
void Cronet_EngineParams_enable_brotli_set(Cronet_EngineParamsPtr, bool);
void Cronet_EngineParams_http_cache_mode_set(Cronet_EngineParamsPtr, Cronet_EngineParams_HTTP_CACHE_MODE);
void Cronet_EngineParams_http_cache_max_size_set(Cronet_EngineParamsPtr, int64_t);
void Cronet_EngineParams_experimental_options_set(Cronet_EngineParamsPtr, Cronet_String);
Cronet_String Cronet_EngineParams_experimental_options_get(Cronet_EngineParamsPtr);
void Cronet_EngineParams_enable_check_result_set(Cronet_EngineParamsPtr, bool);
void Cronet_EngineParams_enable_public_key_pinning_bypass_for_local_trust_anchors_set(
    Cronet_EngineParamsPtr, bool);
void Cronet_EngineParams_network_thread_priority_set(Cronet_EngineParamsPtr, double);
void Cronet_EngineParams_quic_hints_add(Cronet_EngineParamsPtr, Cronet_QuicHintPtr);
void Cronet_EngineParams_public_key_pins_add(Cronet_EngineParamsPtr, Cronet_PublicKeyPinsPtr);

Cronet_QuicHintPtr Cronet_QuicHint_Create(void);
void Cronet_QuicHint_Destroy(Cronet_QuicHintPtr);
void Cronet_QuicHint_host_set(Cronet_QuicHintPtr, Cronet_String);
void Cronet_QuicHint_port_set(Cronet_QuicHintPtr, int32_t);
void Cronet_QuicHint_alternate_port_set(Cronet_QuicHintPtr, int32_t);

Cronet_PublicKeyPinsPtr Cronet_PublicKeyPins_Create(void);
void Cronet_PublicKeyPins_Destroy(Cronet_PublicKeyPinsPtr);
void Cronet_PublicKeyPins_host_set(Cronet_PublicKeyPinsPtr, Cronet_String);
void Cronet_PublicKeyPins_pins_sha256_add(Cronet_PublicKeyPinsPtr, Cronet_String);
void Cronet_PublicKeyPins_include_subdomains_set(Cronet_PublicKeyPinsPtr, bool);
void Cronet_PublicKeyPins_expiration_date_set(Cronet_PublicKeyPinsPtr, int64_t);

Cronet_UrlRequestParamsPtr Cronet_UrlRequestParams_Create(void);
void Cronet_UrlRequestParams_Destroy(Cronet_UrlRequestParamsPtr);
void Cronet_UrlRequestParams_http_method_set(Cronet_UrlRequestParamsPtr, Cronet_String);
void Cronet_UrlRequestParams_disable_cache_set(Cronet_UrlRequestParamsPtr, bool);
void Cronet_UrlRequestParams_priority_set(Cronet_UrlRequestParamsPtr, Cronet_UrlRequestParams_REQUEST_PRIORITY);
void Cronet_UrlRequestParams_allow_direct_executor_set(Cronet_UrlRequestParamsPtr, bool);
void Cronet_UrlRequestParams_request_headers_add(Cronet_UrlRequestParamsPtr, Cronet_HttpHeaderPtr);
void Cronet_UrlRequestParams_upload_data_provider_set(Cronet_UrlRequestParamsPtr, Cronet_UploadDataProviderPtr);
void Cronet_UrlRequestParams_upload_data_provider_executor_set(Cronet_UrlRequestParamsPtr, Cronet_ExecutorPtr);

Cronet_HttpHeaderPtr Cronet_HttpHeader_Create(void);
void Cronet_HttpHeader_Destroy(Cronet_HttpHeaderPtr);
void Cronet_HttpHeader_name_set(Cronet_HttpHeaderPtr, Cronet_String);
void Cronet_HttpHeader_value_set(Cronet_HttpHeaderPtr, Cronet_String);
Cronet_String Cronet_HttpHeader_name_get(Cronet_HttpHeaderPtr);
Cronet_String Cronet_HttpHeader_value_get(Cronet_HttpHeaderPtr);

Cronet_BufferPtr Cronet_Buffer_Create(void);
void Cronet_Buffer_Destroy(Cronet_BufferPtr);
void Cronet_Buffer_InitWithAlloc(Cronet_BufferPtr, uint64_t);
uint64_t Cronet_Buffer_GetSize(Cronet_BufferPtr);
Cronet_RawDataPtr Cronet_Buffer_GetData(Cronet_BufferPtr);

Cronet_UrlRequestPtr Cronet_UrlRequest_Create(void);
void Cronet_UrlRequest_Destroy(Cronet_UrlRequestPtr);
Cronet_RESULT Cronet_UrlRequest_InitWithParams(Cronet_UrlRequestPtr, Cronet_EnginePtr,
    Cronet_String, Cronet_UrlRequestParamsPtr, Cronet_UrlRequestCallbackPtr, Cronet_ExecutorPtr);
Cronet_RESULT Cronet_UrlRequest_Start(Cronet_UrlRequestPtr);
Cronet_RESULT Cronet_UrlRequest_FollowRedirect(Cronet_UrlRequestPtr);
Cronet_RESULT Cronet_UrlRequest_Read(Cronet_UrlRequestPtr, Cronet_BufferPtr);
void Cronet_UrlRequest_Cancel(Cronet_UrlRequestPtr);
bool Cronet_UrlRequest_IsDone(Cronet_UrlRequestPtr);
void Cronet_UrlRequest_GetStatus(Cronet_UrlRequestPtr, Cronet_UrlRequestStatusListenerPtr);

typedef void (*Cronet_Runnable_RunFunc)(Cronet_RunnablePtr);
void Cronet_Runnable_Run(Cronet_RunnablePtr);
void Cronet_Runnable_Destroy(Cronet_RunnablePtr);

typedef void (*Cronet_Executor_ExecuteFunc)(Cronet_ExecutorPtr, Cronet_RunnablePtr);
Cronet_ExecutorPtr Cronet_Executor_CreateWith(Cronet_Executor_ExecuteFunc);
void Cronet_Executor_Destroy(Cronet_ExecutorPtr);
void Cronet_Executor_SetClientContext(Cronet_ExecutorPtr, Cronet_ClientContext);
Cronet_ClientContext Cronet_Executor_GetClientContext(Cronet_ExecutorPtr);

typedef void (*Cronet_UrlRequestCallback_OnRedirectReceivedFunc)(
    Cronet_UrlRequestCallbackPtr, Cronet_UrlRequestPtr, Cronet_UrlResponseInfoPtr, Cronet_String);
typedef void (*Cronet_UrlRequestCallback_OnResponseStartedFunc)(
    Cronet_UrlRequestCallbackPtr, Cronet_UrlRequestPtr, Cronet_UrlResponseInfoPtr);
typedef void (*Cronet_UrlRequestCallback_OnReadCompletedFunc)(
    Cronet_UrlRequestCallbackPtr, Cronet_UrlRequestPtr, Cronet_UrlResponseInfoPtr,
    Cronet_BufferPtr, uint64_t);
typedef void (*Cronet_UrlRequestCallback_OnSucceededFunc)(
    Cronet_UrlRequestCallbackPtr, Cronet_UrlRequestPtr, Cronet_UrlResponseInfoPtr);
typedef void (*Cronet_UrlRequestCallback_OnFailedFunc)(
    Cronet_UrlRequestCallbackPtr, Cronet_UrlRequestPtr, Cronet_UrlResponseInfoPtr, Cronet_ErrorPtr);
typedef void (*Cronet_UrlRequestCallback_OnCanceledFunc)(
    Cronet_UrlRequestCallbackPtr, Cronet_UrlRequestPtr, Cronet_UrlResponseInfoPtr);
Cronet_UrlRequestCallbackPtr Cronet_UrlRequestCallback_CreateWith(
    Cronet_UrlRequestCallback_OnRedirectReceivedFunc,
    Cronet_UrlRequestCallback_OnResponseStartedFunc,
    Cronet_UrlRequestCallback_OnReadCompletedFunc,
    Cronet_UrlRequestCallback_OnSucceededFunc,
    Cronet_UrlRequestCallback_OnFailedFunc,
    Cronet_UrlRequestCallback_OnCanceledFunc);
void Cronet_UrlRequestCallback_Destroy(Cronet_UrlRequestCallbackPtr);
void Cronet_UrlRequestCallback_SetClientContext(Cronet_UrlRequestCallbackPtr, Cronet_ClientContext);
Cronet_ClientContext Cronet_UrlRequestCallback_GetClientContext(Cronet_UrlRequestCallbackPtr);

Cronet_String Cronet_UrlResponseInfo_url_get(Cronet_UrlResponseInfoPtr);
uint32_t Cronet_UrlResponseInfo_url_chain_size(Cronet_UrlResponseInfoPtr);
Cronet_String Cronet_UrlResponseInfo_url_chain_at(Cronet_UrlResponseInfoPtr, uint32_t);
int32_t Cronet_UrlResponseInfo_http_status_code_get(Cronet_UrlResponseInfoPtr);
Cronet_String Cronet_UrlResponseInfo_http_status_text_get(Cronet_UrlResponseInfoPtr);
uint32_t Cronet_UrlResponseInfo_all_headers_list_size(Cronet_UrlResponseInfoPtr);
Cronet_HttpHeaderPtr Cronet_UrlResponseInfo_all_headers_list_at(Cronet_UrlResponseInfoPtr, uint32_t);
bool Cronet_UrlResponseInfo_was_cached_get(Cronet_UrlResponseInfoPtr);
Cronet_String Cronet_UrlResponseInfo_negotiated_protocol_get(Cronet_UrlResponseInfoPtr);
Cronet_String Cronet_UrlResponseInfo_proxy_server_get(Cronet_UrlResponseInfoPtr);
int64_t Cronet_UrlResponseInfo_received_byte_count_get(Cronet_UrlResponseInfoPtr);

Cronet_Error_ERROR_CODE Cronet_Error_error_code_get(Cronet_ErrorPtr);
Cronet_String Cronet_Error_message_get(Cronet_ErrorPtr);
int32_t Cronet_Error_internal_error_code_get(Cronet_ErrorPtr);
bool Cronet_Error_immediately_retryable_get(Cronet_ErrorPtr);
int32_t Cronet_Error_quic_detailed_error_code_get(Cronet_ErrorPtr);

typedef int64_t (*Cronet_UploadDataProvider_GetLengthFunc)(Cronet_UploadDataProviderPtr);
typedef void (*Cronet_UploadDataProvider_ReadFunc)(
    Cronet_UploadDataProviderPtr, Cronet_UploadDataSinkPtr, Cronet_BufferPtr);
typedef void (*Cronet_UploadDataProvider_RewindFunc)(
    Cronet_UploadDataProviderPtr, Cronet_UploadDataSinkPtr);
typedef void (*Cronet_UploadDataProvider_CloseFunc)(Cronet_UploadDataProviderPtr);
Cronet_UploadDataProviderPtr Cronet_UploadDataProvider_CreateWith(
    Cronet_UploadDataProvider_GetLengthFunc,
    Cronet_UploadDataProvider_ReadFunc,
    Cronet_UploadDataProvider_RewindFunc,
    Cronet_UploadDataProvider_CloseFunc);
void Cronet_UploadDataProvider_Destroy(Cronet_UploadDataProviderPtr);
void Cronet_UploadDataProvider_SetClientContext(
    Cronet_UploadDataProviderPtr, Cronet_ClientContext);
Cronet_ClientContext Cronet_UploadDataProvider_GetClientContext(Cronet_UploadDataProviderPtr);
void Cronet_UploadDataSink_OnReadSucceeded(Cronet_UploadDataSinkPtr, uint64_t, bool);
void Cronet_UploadDataSink_OnReadError(Cronet_UploadDataSinkPtr, Cronet_String);
void Cronet_UploadDataSink_OnRewindSucceeded(Cronet_UploadDataSinkPtr);
void Cronet_UploadDataSink_OnRewindError(Cronet_UploadDataSinkPtr, Cronet_String);

#ifdef __cplusplus
}
#endif
#endif
