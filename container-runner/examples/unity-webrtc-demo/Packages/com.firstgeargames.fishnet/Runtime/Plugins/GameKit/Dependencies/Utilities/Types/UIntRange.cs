namespace GameKit.Dependencies.Utilities.Types
{
    [System.Serializable]
    public struct UIntRange
    {
        public UIntRange(uint minimum, uint maximum)
        {
            Minimum = minimum;
            Maximum = maximum;
        }

        /// <summary>
        /// Minimum range.
        /// </summary>
        public uint Minimum;
        /// <summary>
        /// Maximum range.
        /// </summary>
        public uint Maximum;

        /// <summary>
        /// Returns an exclusive random value between Minimum and Maximum.
        /// </summary>
        /// <returns></returns>
        public uint RandomExclusive()
        {
            return UInts.RandomExclusiveRange(Minimum, Maximum);
        }

        /// <summary>
        /// Returns an inclusive random value between Minimum and Maximum.
        /// </summary>
        /// <returns></returns>
        public uint RandomInclusive() => UInts.RandomInclusiveRange(Minimum, Maximum);

        /// <summary>
        /// Clamps value between Minimum and Maximum.
        /// </summary>
        public uint Clamp(uint value) => UInts.Clamp(value, Minimum, Maximum);

        /// <summary>
        /// True if value is within range of Minimum and Maximum.
        /// </summary>
        public bool InRange(uint value) => value >= Minimum && value <= Maximum;
    }
}