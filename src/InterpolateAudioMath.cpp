/**********************************************************************

   Rustacity: A Digital Audio Editor

   InterpolateAudioMath.cpp

   Dominic Mazzoni

   See InterpolateAudioMath.h: this is Matrix.cpp and the arithmetic of
   InterpolateAudio.cpp in one file, free of wxWidgets.

***********************************************************************/

#include "InterpolateAudioMath.h"

#include <algorithm>
#include <cassert>
#include <cmath>
#include <cstdlib>

namespace interp {

Vector::Vector()
{
}

Vector::Vector(unsigned len, double *data)
   : mN{ len }
   , mData(len)
{
   if (data)
      std::copy(data, data + len, mData.data());
   else
      std::fill(mData.data(), mData.data() + len, 0.0);
}

Vector::Vector(unsigned len, float *data)
   : mN{ len }
   , mData(len)
{
   if (data)
      std::copy(data, data + len, mData.data());
   else
      std::fill(mData.data(), mData.data() + len, 0.0);
}

Vector& Vector::operator=(const Vector &other)
{
   assert(Len() == other.Len());
   std::copy(other.mData.data(), other.mData.data() + mN, mData.data());
   return *this;
}

Vector::Vector(const Vector &other)
   : mN{ other.Len() }
   , mData( mN )
{
   std::copy(other.mData.data(), other.mData.data() + mN, mData.data());
}

Vector::~Vector()
{
}

void Vector::Reinit(unsigned len)
{
   Vector temp(len);
   Swap(temp);
}

void Vector::Swap(Vector &that)
{
   std::swap(mN, that.mN);
   mData.swap(that.mData);
}

double Vector::Sum() const
{
   double sum = 0.0;
   for(unsigned i = 0; i < Len(); i++)
      sum += mData[i];
   return sum;
}

Matrix::Matrix(unsigned rows, unsigned cols, double **data)
   : mRows{ rows }
   , mCols{ cols }
   , mRowVec( mRows )
{
   for(unsigned i = 0; i < mRows; i++) {
      mRowVec[i].Reinit( mCols );
      for(unsigned j = 0; j < mCols; j++) {
         if (data)
            (*this)[i][j] = data[i][j];
         else
            (*this)[i][j] = 0.0;
      }
   }
}

Matrix& Matrix::operator=(const Matrix &other)
{
   CopyFrom(other);
   return *this;
}

Matrix::Matrix(const Matrix &other)
{
   CopyFrom(other);
}

void Matrix::CopyFrom(const Matrix &other)
{
   mRows = other.mRows;
   mCols = other.mCols;
   mRowVec.resize(mRows);
   for (unsigned i = 0; i < mRows; i++) {
      mRowVec[i].Reinit( mCols );
      mRowVec[i] = other.mRowVec[i];
   }
}

Matrix::~Matrix()
{
}

void Matrix::SwapRows(unsigned i, unsigned j)
{
   mRowVec[i].Swap(mRowVec[j]);
}

Matrix IdentityMatrix(unsigned N)
{
   Matrix M(N, N);
   for(unsigned i = 0; i < N; i++)
      M[i][i] = 1.0;
   return M;
}

Vector operator+(const Vector &left, const Vector &right)
{
   assert(left.Len() == right.Len());
   Vector v(left.Len());
   for(unsigned i = 0; i < left.Len(); i++)
      v[i] = left[i] + right[i];
   return v;
}

Vector operator-(const Vector &left, const Vector &right)
{
   assert(left.Len() == right.Len());
   Vector v(left.Len());
   for(unsigned i = 0; i < left.Len(); i++)
      v[i] = left[i] - right[i];
   return v;
}

Vector operator*(const Vector &left, const Vector &right)
{
   assert(left.Len() == right.Len());
   Vector v(left.Len());
   for(unsigned i = 0; i < left.Len(); i++)
      v[i] = left[i] * right[i];
   return v;
}

Vector operator*(const Vector &left, double right)
{
   Vector v(left.Len());
   for(unsigned i = 0; i < left.Len(); i++)
      v[i] = left[i] * right;
   return v;
}

Vector VectorSubset(const Vector &other, unsigned start, unsigned len)
{
   Vector v(len);
   for(unsigned i = 0; i < len; i++)
      v[i] = other[start+i];
   return v;
}

Vector VectorConcatenate(const Vector& left, const Vector& right)
{
   Vector v(left.Len() + right.Len());
   for(unsigned i = 0; i < left.Len(); i++)
      v[i] = left[i];
   for(unsigned i = 0; i < right.Len(); i++)
      v[i + left.Len()] = right[i];
   return v;
}

Vector operator*(const Vector &left, const Matrix &right)
{
   assert(left.Len() == right.Rows());
   Vector v(right.Cols());
   for(unsigned i = 0; i < right.Cols(); i++) {
      v[i] = 0.0;
      for(unsigned j = 0; j < right.Rows(); j++)
         v[i] += left[j] * right[j][i];
   }
   return v;
}

Vector operator*(const Matrix &left, const Vector &right)
{
   assert(left.Cols() == right.Len());
   Vector v(left.Rows());
   for(unsigned i = 0; i < left.Rows(); i++) {
      v[i] = 0.0;
      for(unsigned j = 0; j < left.Cols(); j++)
         v[i] += left[i][j] * right[j];
   }
   return v;
}

Matrix operator+(const Matrix &left, const Matrix &right)
{
   assert(left.Rows() == right.Rows());
   assert(left.Cols() == right.Cols());
   Matrix M(left.Rows(), left.Cols());
   for(unsigned i = 0; i < left.Rows(); i++)
      for(unsigned j = 0; j < left.Cols(); j++)
         M[i][j] = left[i][j] + right[i][j];
   return M;
}

Matrix operator*(const Matrix &left, const double right)
{
   Matrix M(left.Rows(), left.Cols());
   for(unsigned i = 0; i < left.Rows(); i++)
      for(unsigned j = 0; j < left.Cols(); j++)
         M[i][j] = left[i][j] * right;
   return M;
}

Matrix ScalarMultiply(const Matrix &left, const Matrix &right)
{
   assert(left.Rows() == right.Rows());
   assert(left.Cols() == right.Cols());
   Matrix M(left.Rows(), left.Cols());
   for(unsigned i = 0; i < left.Rows(); i++)
      for(unsigned j = 0; j < left.Cols(); j++)
         M[i][j] = left[i][j] * right[i][j];
   return M;
}

Matrix MatrixMultiply(const Matrix &left, const Matrix &right)
{
   assert(left.Cols() == right.Rows());
   Matrix M(left.Rows(), right.Cols());
   for(unsigned i = 0; i < left.Rows(); i++)
      for(unsigned j = 0; j < right.Cols(); j++) {
         M[i][j] = 0.0;
         for(unsigned k = 0; k < left.Cols(); k++)
            M[i][j] += left[i][k] * right[k][j];
      }
   return M;
}

Matrix MatrixSubset(const Matrix &input,
                    unsigned startRow, unsigned numRows,
                    unsigned startCol, unsigned numCols)
{
   Matrix M(numRows, numCols);
   for(unsigned i = 0; i < numRows; i++)
      for(unsigned j = 0; j < numCols; j++)
         M[i][j] = input[startRow+i][startCol+j];
   return M;
}

Matrix MatrixConcatenateCols(const Matrix& left, const Matrix& right)
{
   assert(left.Rows() == right.Rows());
   Matrix M(left.Rows(), left.Cols() + right.Cols());
   for(unsigned i = 0; i < left.Rows(); i++) {
      for(unsigned j = 0; j < left.Cols(); j++)
         M[i][j] = left[i][j];
      for(unsigned j = 0; j < right.Cols(); j++)
         M[i][j+left.Cols()] = right[i][j];
   }
   return M;
}

Matrix TransposeMatrix(const Matrix& other)
{
   Matrix M(other.Cols(), other.Rows());
   for(unsigned i = 0; i < other.Rows(); i++)
      for(unsigned j = 0; j < other.Cols(); j++)
         M[j][i] = other[i][j];
   return M;
}

bool InvertMatrix(const Matrix& input, Matrix& Minv)
{
   // Very straightforward implementation of
   // Gauss-Jordan elimination to invert a matrix.
   // Returns true if successful

   assert(input.Rows() == input.Cols());
   auto N = input.Rows();

   Matrix M = input;
   Minv = IdentityMatrix(N);

   // Do the elimination one column at a time
   for(unsigned i = 0; i < N; i++) {
      // Pivot the row with the largest absolute value in
      // column i, into row i
      double absmax = 0.0;
      unsigned int argmax = 0;

      for(unsigned j = i; j < N; j++)
         if (fabs(M[j][i]) > absmax) {
            absmax = fabs(M[j][i]);
            argmax = j;
         }

      // If no row has a nonzero value in that column,
      // the matrix is singular and we have to give up.
      if (absmax == 0)
         return false;

      if (i != argmax) {
         M.SwapRows(i, argmax);
         Minv.SwapRows(i, argmax);
      }

      // Divide this row by the value of M[i][i]
      double factor = 1.0 / M[i][i];
      M[i] = M[i] * factor;
      Minv[i] = Minv[i] * factor;

      // Eliminate the rest of the column
      for(unsigned j = 0; j < N; j++) {
         if (j == i)
            continue;
         if (fabs(M[j][i]) > 0) {
            // Subtract a multiple of row i from row j
            factor = M[j][i];
            for(unsigned k = 0; k < N; k++) {
               M[j][k] -= (M[i][k] * factor);
               Minv[j][k] -= (Minv[i][k] * factor);
            }
         }
      }
   }

   return true;
}

static inline int imin(int x, int y)
{
   return x<y? x: y;
}

static inline int imax(int x, int y)
{
   return x>y? x: y;
}

// This function is a really dumb, simple way to interpolate audio,
// if the more general InterpolateAudio function below doesn't have
// enough data to work with.  If the bad samples are in the middle,
// it's literally linear.  If it's on either edge, we add some decay
// back to zero.
static void LinearInterpolateAudio(float *buffer, int len,
                                   int firstBad, int numBad)
{
   int i;

   float decay = 0.9f;

   if (firstBad==0) {
      float delta = buffer[numBad] - buffer[numBad+1];
      float value = buffer[numBad];
      i = numBad - 1;
      while (i >= 0) {
         value += delta;
         buffer[i] = value;
         value *= decay;
         delta *= decay;
         i--;
      }
   }
   else if (firstBad + numBad == len) {
      float delta = buffer[firstBad-1] - buffer[firstBad-2];
      float value = buffer[firstBad-1];
      i = firstBad;
      while (i < firstBad + numBad) {
         value += delta;
         buffer[i] = value;
         value *= decay;
         delta *= decay;
         i++;
      }
   }
   else {
      float v1 = buffer[firstBad-1];
      float v2 = buffer[firstBad+numBad];
      float value = v1;
      float delta = (v2 - v1) / (numBad+1);
      i = firstBad;
      while (i < firstBad + numBad) {
         value += delta;
         buffer[i] = value;
         i++;
      }
   }
}

// Here's the main interpolate function, using
// Least Squares AutoRegression (LSAR):
void InterpolateAudio(float *buffer, const size_t len,
                      size_t firstBad, size_t numBad)
{
   const auto N = len;

   assert(len > 0 &&
            numBad < len &&
            firstBad+numBad <= len);

   if(numBad >= len)
      return;  //should never have been called!

   if (firstBad == 0) {
      // The algorithm below has a weird asymmetry in that it
      // performs poorly when interpolating to the left.  If
      // we're asked to interpolate the left side of a buffer,
      // we just reverse the problem and try it that way.
      std::vector<float> buffer2( len );
      for(size_t i=0; i<len; i++)
         buffer2[len-1-i] = buffer[i];
      InterpolateAudio(buffer2.data(), len, len-numBad, numBad);
      for(size_t i=0; i<len; i++)
         buffer[len-1-i] = buffer2[i];
      return;
   }

   Vector s(len, buffer);

   // Choose P, the order of the autoregression equation
   const int IP =
      imin(imin(numBad * 3, 50), imax(firstBad - 1, len - (firstBad + numBad) - 1));

   if (IP < 3 || IP >= (int)N) {
      LinearInterpolateAudio(buffer, len, firstBad, numBad);
      return;
   }

   size_t P(IP);

   // Add a tiny amount of random noise to the input signal -
   // this sounds like a bad idea, but the amount we're adding
   // is only about 1 bit in 16-bit audio, and it's an extremely
   // effective way to avoid nearly-singular matrices.  If users
   // run it more than once they get slightly different results;
   // this is sometimes even advantageous.
   for(size_t i=0; i<N; i++)
      s[i] += (rand()-(RAND_MAX/2))/(RAND_MAX*10000.0);

   // Solve for the best autoregression coefficients
   // using a least-squares fit to all of the non-bad
   // data we have in the buffer
   Matrix X(P, P);
   Vector b(P);

   for(size_t i = 0; i + P < len; i++)
      if (i+P < firstBad || i >= (firstBad + numBad))
         for(size_t row=0; row<P; row++) {
            for(size_t col=0; col<P; col++)
               X[row][col] += (s[i+row] * s[i+col]);
            b[row] += s[i+P] * s[i+row];
         }

   Matrix Xinv(P, P);
   if (!InvertMatrix(X, Xinv)) {
      // The matrix is singular!  Fall back on linear...
      // In practice I have never seen this happen if
      // we add the tiny bit of random noise.
      LinearInterpolateAudio(buffer, len, firstBad, numBad);
      return;
   }

   // This vector now contains the autoregression coefficients
   const Vector &a = Xinv * b;

   // Create a matrix (a "Toeplitz" matrix, as it turns out)
   // which encodes the autoregressive relationship between
   // elements of the sequence.
   Matrix A(N-P, N);
   for(size_t row=0; row<N-P; row++) {
      for(size_t col=0; col<P; col++)
         A[row][row+col] = -a[col];
      A[row][row+P] = 1;
   }

   // Split both the Toeplitz matrix and the signal into
   // two pieces.  Note that this code could be made to
   // work even in the case where the "bad" samples are
   // not contiguous, but currently it assumes they are.
   //   "u" is for unknown (bad)
   //   "k" is for known (good)
   Matrix Au = MatrixSubset(A, 0, N-P, firstBad, numBad);
   Matrix A_left = MatrixSubset(A, 0, N-P, 0, firstBad);
   Matrix A_right = MatrixSubset(A, 0, N-P,
                                 firstBad+numBad, N-(firstBad+numBad));
   Matrix Ak = MatrixConcatenateCols(A_left, A_right);

   const Vector &s_left = VectorSubset(s, 0, firstBad);
   const Vector &s_right = VectorSubset(s, firstBad+numBad,
                                 N-(firstBad+numBad));
   const Vector &sk = VectorConcatenate(s_left, s_right);

   // Do some linear algebra to find the best possible
   // values that fill in the "bad" area
   Matrix AuT = TransposeMatrix(Au);
   Matrix X1 = MatrixMultiply(AuT, Au);
   Matrix X2(X1.Rows(), X1.Cols());
   if (!InvertMatrix(X1, X2)) {
      // The matrix is singular!  Fall back on linear...
      LinearInterpolateAudio(buffer, len, firstBad, numBad);
      return;
   }
   Matrix X2b = X2 * -1.0;
   Matrix X3 = MatrixMultiply(X2b, AuT);
   Matrix X4 = MatrixMultiply(X3, Ak);
   // This vector contains our best guess as to the
   // unknown values
   const Vector &su = X4 * sk;

   // Put the results into the return buffer
   for(size_t i=0; i<numBad; i++)
      buffer[firstBad+i] = (float)su[i];
}

} // namespace interp
