using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public static class Quaternions
    {
        /// <summary>
        /// Returns how fast an object must rotate over duration to reach goal.
        /// </summary>
        /// <param name = "goal">Quaternion to measure distance against.</param>
        /// <param name = "duration">How long it should take to move to goal.</param>
        /// <param name = "interval">A multiplier applied towards interval. Typically this is used for ticks passed.</param>
        /// <returns></returns>
        public static float GetRate(this Quaternion a, Quaternion goal, float duration, out float angle, uint interval = 1, float tolerance = 0f)
        {
            angle = a.Angle(goal, true);
            return angle / (duration * interval);
        }

        /// <summary>
        /// Subtracts b quaternion from a.
        /// </summary>
        public static Quaternion Subtract(this Quaternion a, Quaternion b) => Quaternion.Inverse(b) * a;

        /// <summary>
        /// Adds quaternion b onto quaternion a.
        /// </summary>
        public static Quaternion Add(this Quaternion a, Quaternion b) => a * b;

        /// <summary>
        /// Returns if two quaternions match.
        /// </summary>
        /// <param name = "precise">True to use a custom implementation with no error tolerance. False to use Unity's implementation which may return a match even when not true due to error tolerance.</param>
        /// <returns></returns>
        public static bool Matches(this Quaternion a, Quaternion b, bool precise = false)
        {
            if (precise)
                return a.w == b.w && a.x == b.x && a.y == b.y && a.z == b.z;
            else
                return a == b;
        }

        /// <summary>
        /// Returns the angle between two quaterions.
        /// </summary>
        /// <param name = "precise">True to use a custom implementation with no error tolerance. False to use Unity's implementation which may return 0f due to error tolerance, even while there is a difference.</param>
        /// <returns></returns>
        public static float Angle(this Quaternion a, Quaternion b, bool precise = false)
        {
            if (precise)
            {
                // This is run Unitys implementation without the error tolerance.
                float dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
                return Mathf.Acos(Mathf.Min(Mathf.Abs(dot), 1f)) * 2f * 57.29578f;
            }
            else
            {
                return Quaternion.Angle(a, b);
            }
        }
    }
}