using System.Runtime.CompilerServices;

namespace GameKit.Dependencies.Utilities
{
    /// <summary>
    /// Various utility classes relating to floats.
    /// </summary>
    public static class UInts
    {
        /// <summary>
        /// Pads an index a specified value. Preferred over typical padding so that pad values used with skins can be easily found in the code.
        /// </summary>
        public static string Pad(this uint value, int padding)
        {
            if (padding < 0)
                padding = 0;
            return value.ToString().PadLeft(padding, '0');
        }

        /// <summary>
        /// Provides a random inclusive int within a given range. Preferred over Unity's Random to eliminate confusion as Unity uses inclusive for floats max, and exclusive for int max.
        /// </summary>
        /// <param name = "minimum">Inclusive minimum value.</param>
        /// <param name = "maximum">Inclusive maximum value.</param>
        /// <returns></returns>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static uint RandomInclusiveRange(uint minimum, uint maximum) => (uint)Ints.RandomInclusiveRange((int)minimum, (int)maximum);

        /// <summary>
        /// Provides a random exclusive int within a given range. Preferred over Unity's Random to eliminate confusion as Unity uses inclusive for floats max, and exclusive for int max.
        /// </summary>
        /// <param name = "minimum">Inclusive minimum value.</param>
        /// <param name = "maximum">Exclusive maximum value.</param>
        /// <returns></returns>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static uint RandomExclusiveRange(uint minimum, uint maximum) => (uint)Ints.RandomExclusiveRange((int)minimum, (int)maximum);

        /// <summary>
        /// Returns a clamped int within a specified range.
        /// </summary>
        /// <param name = "value">Value to clamp.</param>
        /// <param name = "minimum">Minimum value.</param>
        /// <param name = "maximum">Maximum value.</param>
        /// <returns></returns>
        public static uint Clamp(uint value, uint minimum, uint maximum)
        {
            if (value < minimum)
                value = minimum;
            else if (value > maximum)
                value = maximum;

            return value;
        }

        /// <summary>
        /// Returns whichever value is lower.
        /// </summary>
        public static uint Min(uint a, uint b) => a < b ? a : b;

        /// <summary>
        /// Determins if all values passed in are the same.
        /// </summary>
        /// <param name = "values">Values to check.</param>
        /// <returns>True if all values are the same.</returns>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static bool ValuesMatch(params uint[] values) => Ints.ValuesMatch((int[])(object)values);
    }
}