using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    /// <summary>
    /// Various utility classes relating to floats.
    /// </summary>
    public static class Ints
    {
        private static System.Random _random = new();

        /// <summary>
        /// Pads an index a specified value. Preferred over typical padding so that pad values used with skins can be easily found in the code.
        /// </summary>
        /// <param name = "value"></param>
        /// <param name = "padding"></param>
        /// <returns></returns>
        public static string PadInt(int value, int padding)
        {
            return value.ToString().PadLeft(padding, '0');
        }

        /// <summary>
        /// Provides a random inclusive int within a given range. Preferred over Unity's Random to eliminate confusion as Unity uses inclusive for floats max, and exclusive for int max.
        /// </summary>
        /// <param name = "minimum">Inclusive minimum value.</param>
        /// <param name = "maximum">Inclusive maximum value.</param>
        /// <returns></returns>
        public static int RandomInclusiveRange(int minimum, int maximum)
        {
            return _random.Next(minimum, maximum + 1);
        }

        /// <summary>
        /// Provides a random exclusive int within a given range. Preferred over Unity's Random to eliminate confusion as Unity uses inclusive for floats max, and exclusive for int max.
        /// </summary>
        /// <param name = "minimum">Inclusive minimum value.</param>
        /// <param name = "maximum">Exclusive maximum value.</param>
        /// <returns></returns>
        public static int RandomExclusiveRange(int minimum, int maximum)
        {
            return _random.Next(minimum, maximum);
        }

        /// <summary>
        /// Returns a clamped int within a specified range.
        /// </summary>
        /// <param name = "value">Value to clamp.</param>
        /// <param name = "minimum">Minimum value.</param>
        /// <param name = "maximum">Maximum value.</param>
        /// <returns></returns>
        public static int Clamp(int value, int minimum, int maximum)
        {
            if (value < minimum)
                value = minimum;
            else if (value > maximum)
                value = maximum;

            return value;
        }

        /// <summary>
        /// Determins if all values passed in are the same.
        /// </summary>
        /// <param name = "values">Values to check.</param>
        /// <returns>True if all values are the same.</returns>
        public static bool ValuesMatch(params int[] values)
        {
            if (values.Length == 0)
            {
                Debug.Log("Ints -> ValuesMatch -> values array is empty.");
                return false;
            }

            // Assign first value as element in first array.
            int firstValue = values[0];
            // Check all values.
            for (int i = 1; i < values.Length; i++)
            {
                // If any value doesn't match first value return false.
                if (firstValue != values[i])
                    return false;
            }

            // If this far all values match.
            return true;
        }
    }
}