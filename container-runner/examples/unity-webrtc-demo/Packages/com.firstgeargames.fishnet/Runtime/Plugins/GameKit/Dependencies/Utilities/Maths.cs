namespace GameKit.Dependencies.Utilities
{
    public static class Maths
    {
        /// <summary>
        /// Returns a clamped SBytte.
        /// </summary>
        public static sbyte ClampSByte(long value, sbyte min, sbyte max)
        {
            if (value < min)
                return min;
            else if (value > max)
                return max;
            else
                return (sbyte)value;
        }

        /// <summary>
        /// Returns a clamped double.
        /// </summary>
        public static double ClampDouble(double value, double min, double max)
        {
            if (value < min)
                return min;
            else if (value > max)
                return max;
            else
                return value;
        }

        /// <summary>
        /// Returns a clamped byte.
        /// </summary>
        public static byte ClampByte(byte value, byte min, byte max)
        {
            if (value < min)
                return min;
            else if (value > max)
                return max;
            else
                return value;
        }
    }
}