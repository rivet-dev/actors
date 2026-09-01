using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types
{
    [System.Serializable]
    public struct Vector2Range
    {
        public Vector2Range(Vector2 minimum, Vector2 maximum)
        {
            X = new(minimum.x, maximum.x);
            Y = new(minimum.y, maximum.y);
        }

        public Vector2Range(FloatRange minimum, FloatRange maximum)
        {
            X = minimum;
            Y = maximum;
        }

        /// <summary>
        /// Minimum range.
        /// </summary>
        public FloatRange X;
        /// <summary>
        /// Maximum range.
        /// </summary>
        public FloatRange Y;

        /// <summary>
        /// Returns a random value between Minimum and Maximum.
        /// </summary>
        /// <returns></returns>
        public Vector2 RandomInclusive()
        {
            return new(Floats.RandomInclusiveRange(X.Minimum, X.Maximum), Floats.RandomInclusiveRange(Y.Minimum, Y.Maximum));
        }
    }
}