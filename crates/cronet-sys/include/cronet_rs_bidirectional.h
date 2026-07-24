#ifndef CRONET_RS_BIDIRECTIONAL_H_
#define CRONET_RS_BIDIRECTIONAL_H_

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct stream_engine {
  void* obj;
  void* annotation;
} stream_engine;

typedef struct bidirectional_stream {
  void* obj;
  void* annotation;
} bidirectional_stream;

typedef struct bidirectional_stream_header {
  const char* key;
  const char* value;
} bidirectional_stream_header;

typedef struct bidirectional_stream_header_array {
  size_t count;
  size_t capacity;
  bidirectional_stream_header* headers;
} bidirectional_stream_header_array;

typedef struct bidirectional_stream_callback {
  void (*on_stream_ready)(bidirectional_stream*);
  void (*on_response_headers_received)(
      bidirectional_stream*, bidirectional_stream_header_array*, char*);
  void (*on_read_completed)(bidirectional_stream*, char*, int);
  void (*on_write_completed)(bidirectional_stream*, char*);
  void (*on_response_trailers_received)(
      bidirectional_stream*, bidirectional_stream_header_array*);
  void (*on_succeded)(bidirectional_stream*);
  void (*on_failed)(bidirectional_stream*, int);
  void (*on_canceled)(bidirectional_stream*);
} bidirectional_stream_callback;

bidirectional_stream* bidirectional_stream_create(
    stream_engine*, void*, const bidirectional_stream_callback*);
int bidirectional_stream_destroy(bidirectional_stream*);
void bidirectional_stream_disable_auto_flush(bidirectional_stream*, bool);
void bidirectional_stream_delay_request_headers_until_flush(
    bidirectional_stream*, bool);
int bidirectional_stream_start(
    bidirectional_stream*, const char*, int, const char*,
    const bidirectional_stream_header_array*, bool);
int bidirectional_stream_read(bidirectional_stream*, char*, int);
int bidirectional_stream_write(bidirectional_stream*, const char*, int, bool);
void bidirectional_stream_flush(bidirectional_stream*);
void bidirectional_stream_cancel(bidirectional_stream*);

#ifdef __cplusplus
}
#endif
#endif

