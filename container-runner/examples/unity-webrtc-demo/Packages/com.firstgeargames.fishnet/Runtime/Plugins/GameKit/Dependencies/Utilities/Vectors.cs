using System;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public static class Vectors
    {
        /// <summary>
        /// Vector3.zero.
        /// </summary>
        private static readonly Vector3 VECTOR3_ZERO = new(0.0f, 0.0f, 0.0f);
        /// <summary>
        /// Float epislon.
        /// </summary>
        private const float FLOAT_EPSILON = 0.00001f;

        #region Vector3.
        /// <summary>
        /// Returns how fast an object must move over duration to reach goal.
        /// </summary>
        /// <param name = "b">Vector3 to measure distance against.</param>
        /// <param name = "duration">How long it should take to move to goal.</param>
        /// <param name = "interval">A multiplier applied towards interval. Typically this is used for ticks passed.</param>
        /// <returns></returns>
        public static float GetRate(this Vector3 a, Vector3 b, float duration, out float distance, uint interval = 1)
        {
            distance = Vector3.Distance(a, b);
            return distance / (duration * interval);
        }

        /// <summary>
        /// Adds a Vector2 X/Y onto a Vector3.
        /// </summary>
        public static Vector3 Add(this Vector3 v3, Vector2 v2)
        {
            return v3 + new Vector3(v2.x, v2.y, 0f);
        }

        /// <summary>
        /// Subtracts a Vector2 X/Y from a Vector3.
        /// </summary>
        public static Vector3 Subtract(this Vector3 v3, Vector2 v2)
        {
            return v3 - new Vector3(v2.x, v2.y, 0f);
        }

        /// <summary>
        /// Calculates the linear parameter t that produces the interpolant value within the range [a, b].
        /// </summary>
        /// <param name = "a"></param>
        /// <param name = "b"></param>
        /// <param name = "value"></param>
        /// <returns></returns>
        public static float InverseLerp(Vector3 a, Vector3 b, Vector3 value)
        {
            Vector3 ab = b - a;
            Vector3 av = value - a;
            return Mathf.Clamp01(Vector3.Dot(av, ab) / Vector3.Dot(ab, ab));
        }

        /// <summary>
        /// Returns if the target Vector3 is within variance of the source Vector3.
        /// </summary>
        /// <param name = "a">Source vector.</param>
        /// <param name = "b">Target vector.</param>
        /// <param name = "tolerance">How close the target vector must be to be considered close.</param>
        /// <returns></returns>
        public static bool Near(this Vector3 a, Vector3 b, float tolerance = 0.01f)
        {
            return Vector3.Distance(a, b) <= tolerance;
        }

        /// <summary>
        /// Returns if any values within a Vector3 are NaN.
        /// </summary>
        /// <param name = "source"></param>
        /// <returns></returns>
        public static bool IsNan(this Vector3 source)
        {
            return float.IsNaN(source.x) || float.IsNaN(source.y) || float.IsNaN(source.z);
        }

        /// <summary>
        /// Lerp between three Vector3 values.
        /// </summary>
        /// <returns></returns>
        public static Vector3 Lerp3(Vector3 a, Vector3 b, Vector3 c, float percent)
        {
            Vector3 r0 = Vector3.Lerp(a, b, percent);
            Vector3 r1 = Vector3.Lerp(b, c, percent);
            return Vector3.Lerp(r0, r1, percent);
        }

        /// <summary>
        /// Lerp between three Vector3 values.
        /// </summary>
        /// <param name = "vectors"></param>
        /// <param name = "percent"></param>
        /// <returns></returns>
        public static Vector3 Lerp3(Vector3[] vectors, float percent)
        {
            if (vectors.Length < 3)
            {
                Debug.LogWarning("Vectors -> Lerp3 -> Vectors length must be 3.");
                return Vector3.zero;
            }

            return Lerp3(vectors[0], vectors[1], vectors[2], percent);
        }

        /// <summary>
        /// Multiplies a Vector3 by another.
        /// </summary>
        /// <param name = "src"></param>
        /// <param name = "multiplier"></param>
        /// <returns></returns>
        public static Vector3 Multiply(this Vector3 src, Vector3 multiplier)
        {
            return new(src.x * multiplier.x, src.y * multiplier.y, src.z * multiplier.z);
        }

        #region Fast.
        /* Fast checks are property of:
         *  Copyright (c) 2020 Maxim Munnig Schmidt
         *
         *  Permission is hereby granted, free of charge, to any person obtaining a copy
         *  of this software and associated documentation files (the "Software"), to deal
         *  in the Software without restriction, including without limitation the rights
         *  to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
         *  copies of the Software, and to permit persons to whom the Software is
         *  furnished to do so, subject to the following conditions:
         *
         *  The above copyright notice and this permission notice shall be included in all
         *  copies or substantial portions of the Software.
         *
         *  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
         *  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
         *  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
         *  AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
         *  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
         *  OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
         *  SOFTWARE.
         */
        /// <summary>
        /// Fast Distance.
        /// </summary>
        /// <param name = "a"></param>
        /// <param name = "b"></param>
        /// <returns></returns>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static float FastDistance(Vector3 a, Vector3 b)
        {
            float distx = a.x - b.x;
            float disty = a.y - b.y;
            float distz = a.z - b.z;
            return (float)Math.Sqrt(distx * distx + disty * disty + distz * distz);
        }

        /// <summary>
        /// Fast SqrMagnitude.
        /// </summary>
        /// <param name = "vector"></param>
        /// <returns></returns>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static float FastSqrMagnitude(Vector3 vector)
        {
            return vector.x * vector.x + vector.y * vector.y + vector.z * vector.z;
        }

        /// <summary>
        /// Fast Normalize.
        /// </summary>
        /// <param name = "value"></param>
        /// <returns></returns>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static Vector3 FastNormalize(Vector3 value)
        {
            float mag = (float)Math.Sqrt(value.x * value.x + value.y * value.y + value.z * value.z); // Magnitude(value);
            if (mag > FLOAT_EPSILON)
            {
                Vector3 result;
                result.x = value.x / mag;
                result.y = value.y / mag;
                result.z = value.z / mag;
                return result; // value / mag;
            }
            else
            {
                return VECTOR3_ZERO;
            }
        }
        #endregion
        #endregion

        #region Vector2.
        /// <summary>
        /// Returns how fast an object must move over duration to reach goal.
        /// </summary>
        /// <param name = "goal">Vector3 to measure distance against.</param>
        /// <param name = "duration">How long it should take to move to goal.</param>
        /// <param name = "interval">A multiplier applied towards interval. Typically this is used for ticks passed.</param>
        /// <returns></returns>
        public static float GetRate(this Vector2 a, Vector2 goal, float duration, out float distance, uint interval = 1)
        {
            distance = Vector2.Distance(a, goal);
            return distance / (duration * interval);
        }

        /// <summary>
        /// Lerp between three Vector2 values.
        /// </summary>
        /// <param name = "a"></param>
        /// <param name = "b"></param>
        /// <param name = "c"></param>
        /// <param name = "percent"></param>
        /// <returns></returns>
        public static Vector2 Lerp3(Vector2 a, Vector2 b, Vector2 c, float percent)
        {
            Vector2 r0 = Vector2.Lerp(a, b, percent);
            Vector2 r1 = Vector2.Lerp(b, c, percent);
            return Vector2.Lerp(r0, r1, percent);
        }

        /// <summary>
        /// Lerp between three Vector2 values.
        /// </summary>
        /// <param name = "vectors"></param>
        /// <param name = "percent"></param>
        /// <returns></returns>
        public static Vector2 Lerp2(Vector2[] vectors, float percent)
        {
            if (vectors.Length < 3)
            {
                Debug.LogWarning("Vectors -> Lerp3 -> Vectors length must be 3.");
                return Vector2.zero;
            }

            return Lerp3(vectors[0], vectors[1], vectors[2], percent);
        }

        /// <summary>
        /// Multiplies a Vector2 by another.
        /// </summary>
        /// <param name = "src"></param>
        /// <param name = "multiplier"></param>
        /// <returns></returns>
        public static Vector2 Multiply(this Vector2 src, Vector2 multiplier)
        {
            return new(src.x * multiplier.x, src.y * multiplier.y);
        }
        #endregion
    }
}