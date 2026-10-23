/* sqlite-vec uses BSD integer typedefs without including sys/types.h.
 * Force the declaring header into C builds without exposing C declarations to assembly. */
#ifndef __ASSEMBLER__
#include <sys/types.h>
#endif
