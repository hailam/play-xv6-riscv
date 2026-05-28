// Stub pthread.h satisfying bc's `#include <pthread.h>` in
// include/library.h. We don't build libbcl (`BC_ENABLE_LIBRARY=0`)
// so none of the pthread *calls* are ever emitted — only the
// type names are referenced in header declarations.
//
// Picolibc has no pthread.h; this stub sits earlier on the
// include path so bc's `#include <pthread.h>` resolves here.

#ifndef BC_XV6RS_PTHREAD_STUB_H
#define BC_XV6RS_PTHREAD_STUB_H

typedef int pthread_t;
typedef int pthread_key_t;
typedef int pthread_once_t;
typedef int pthread_mutex_t;
typedef int pthread_attr_t;
typedef int pthread_mutexattr_t;

#endif
