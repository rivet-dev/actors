namespace GameKit.Dependencies.Utilities.Types
{
    [System.Serializable]
    public struct IntRange
    {
        public IntRange(int minimum, int maximum)
        {
            Minimum = minimum;
            Maximum = maximum;
        }

        /// <summary>
        /// Minimum range.
        /// </summary>
        public int Minimum;
        /// <summary>
        /// Maximum range.
        /// </summary>
        public int Maximum;

        /// <summary>
        /// Returns an exclusive random value between Minimum and Maximum.
        /// </summary>
        /// <returns></returns>
        public float RandomExclusive()
        {
            return Ints.RandomExclusiveRange(Minimum, Maximum);
        }

        /// <summary>
        /// Returns an inclusive random value between Minimum and Maximum.
        /// </summary>
        /// <returns></returns>
        public float RandomInclusive()
        {
            return Ints.RandomInclusiveRange(Minimum, Maximum);
        }

        /// <summary>
        /// Returns value clamped within minimum and maximum.
        /// </summary>
        /// <param name = "value"></param>
        /// <returns></returns>
        public int Clamp(int value)
        {
            if (value < Minimum)
                return Minimum;
            if (value > Maximum)
                return Maximum;
            return value;
        }
    }
}