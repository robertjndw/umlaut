/* Hand-written replacement for the config.h that Graphviz's configure script
 * generates. Only what the dot layout needs: no plugins on disk, no renderers, no
 * expat (HTML labels), no zlib. Version macros come from the tarball's own header. */
#pragma once

#include "graphviz_version.h"

#define DEFAULT_DPI 96
#define ORTHO 1

#define HAVE_DRAND48 1
#define HAVE_SRAND48 1
#define HAVE_SETENV 1
#define HAVE_STRCASESTR 1
