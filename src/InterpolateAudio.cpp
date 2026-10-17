/**********************************************************************

  Rustacity: A Digital Audio Editor

  InterpolateAudio.cpp

  Dominic Mazzoni

**********************************************************************/

#include "InterpolateAudio.h"

#include "InterpolateAudioMath.h"

// The arithmetic of the interpolation, and the linear algebra it is built on,
// are in InterpolateAudioMath.cpp, which is free of wxWidgets so that the
// golden-value dumpers can compile it on its own.  What is left here is the
// entry point the application calls.
void InterpolateAudio(float *buffer, const size_t len,
                      size_t firstBad, size_t numBad)
{
   interp::InterpolateAudio(buffer, len, firstBad, numBad);
}
