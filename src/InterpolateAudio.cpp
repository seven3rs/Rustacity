/**********************************************************************

  Rustacity: A Digital Audio Editor

  InterpolateAudio.cpp

  Dominic Mazzoni

**********************************************************************/

#include "InterpolateAudio.h"

#include "rustacity_dsp.h"

// The interpolation, and the linear algebra it is built on, live in the Rust
// core (rust/dsp, crate "rustacity-dsp"); this file is the C++ side of the C ABI
// declared in rust/dsp/include/rustacity_dsp.h.  What is left here is the entry
// point the application calls.
//
// The one difference the port makes is that the noise added to keep the solve
// away from singularity comes from a generator of the core's own, started at
// the same place every call, where the C++ drew it from the process-global
// rand(): a repair no longer depends on how much random data everything else
// has drawn, and no longer differs from one run to the next.
void InterpolateAudio(float *buffer, const size_t len,
                      size_t firstBad, size_t numBad)
{
   rustacity_interpolate_audio(buffer, len, firstBad, numBad);
}
