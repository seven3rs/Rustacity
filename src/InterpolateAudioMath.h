/**********************************************************************

   Rustacity: A Digital Audio Editor

   InterpolateAudioMath.h

   Dominic Mazzoni

   The arithmetic of the LSAR audio interpolation, moved out of
   InterpolateAudio.cpp, together with the linear algebra it is built on,
   moved out of Matrix.cpp, so that it can be compiled on its own: no
   wxWidgets, no application headers, nothing but <algorithm>, <cassert>,
   <cmath>, <cstddef>, <cstdlib> and <vector>.  InterpolateAudio.cpp keeps
   the entry point the application calls and Matrix.cpp is gone.

   The move is a copy, not a revision.  The only substitutions are the ones
   wx-freedom forces: wxASSERT becomes assert and the wxWidgets containers
   (Doubles, ArrayOf, Floats) become std::vector.

   The Rust port replaces this file - see rust/dsp/src/interpolate_audio.rs
   and rust/dsp/goldens/capture/.

***********************************************************************/

#ifndef __RUSTACITY_INTERPOLATE_AUDIO_MATH_H__
#define __RUSTACITY_INTERPOLATE_AUDIO_MATH_H__

#include <cstddef>
#include <vector>

namespace interp {

class Matrix;

class Vector
{
 public:
   Vector();
   Vector(const Vector& copyFrom);
   Vector(unsigned len, double *data=nullptr);
   Vector(unsigned len, float *data);
   Vector& operator=(const Vector &other);
   ~Vector();

   void Reinit(unsigned len);
   void Swap(Vector &that);

   inline double& operator[](unsigned i) { return mData[i]; }
   inline double operator[](unsigned i) const { return mData[i]; }
   inline unsigned Len() const { return mN; }

   double Sum() const;

 private:
   unsigned mN{ 0 };
   std::vector<double> mData;
};

class Matrix
{
 public:
   Matrix(const Matrix& copyFrom);
   Matrix(unsigned rows, unsigned cols, double **data=nullptr);
   ~Matrix();

   Matrix& operator=(const Matrix& other);

   inline Vector& operator[](unsigned i) { return mRowVec[i]; }
   inline Vector& operator[](unsigned i) const { return mRowVec[i]; }
   inline unsigned Rows() const { return mRows; }
   inline unsigned Cols() const { return mCols; }

   void SwapRows(unsigned i, unsigned j);

 private:
   void CopyFrom(const Matrix& other);

   unsigned mRows;
   unsigned mCols;
   std::vector<Vector> mRowVec;
};

bool InvertMatrix(const Matrix& input, Matrix& Minv);

Matrix TransposeMatrix(const Matrix& M);

Matrix IdentityMatrix(unsigned N);

Vector operator+(const Vector &left, const Vector &right);
Vector operator-(const Vector &left, const Vector &right);
Vector operator*(const Vector &left, const Vector &right);
Vector operator*(const Vector &left, double right);

Vector VectorSubset(const Vector &other, unsigned start, unsigned len);
Vector VectorConcatenate(const Vector& left, const Vector& right);

Vector operator*(const Vector &left, const Matrix &right);
Vector operator*(const Matrix &left, const Vector &right);

Matrix operator+(const Matrix &left, const Matrix &right);
Matrix operator*(const Matrix &left, const double right);

// No operator* on matrices due to ambiguity
Matrix ScalarMultiply(const Matrix &left, const Matrix &right);
Matrix MatrixMultiply(const Matrix &left, const Matrix &right);

Matrix MatrixSubset(const Matrix &M,
                    unsigned startRow, unsigned numRows,
                    unsigned startCol, unsigned numCols);

Matrix MatrixConcatenateCols(const Matrix& left, const Matrix& right);

// See top of file for a description of the algorithm.  Interpolates
// the samples from buffer[firstBad] through buffer[firstBad+numBad-1],
// ignoring whatever value was there previously, and replacing them with
// values determined from the surrounding audio.
void InterpolateAudio(float *buffer, size_t len,
                      size_t firstBad, size_t numBad);

} // namespace interp

#endif // __RUSTACITY_INTERPOLATE_AUDIO_MATH_H__
