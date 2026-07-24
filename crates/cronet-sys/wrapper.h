#include <stdbool.h>
#include <stdint.h>

#if defined(CRONET_RS_EXTERNAL_HEADER)
#include CRONET_RS_EXTERNAL_HEADER
#else
#include "cronet_rs_dev.h"
#endif

#if __has_include("bidirectional_stream_c.h")
#include "bidirectional_stream_c.h"
#else
#include "cronet_rs_bidirectional.h"
#endif

#include "cronet_rs_naive.h"
