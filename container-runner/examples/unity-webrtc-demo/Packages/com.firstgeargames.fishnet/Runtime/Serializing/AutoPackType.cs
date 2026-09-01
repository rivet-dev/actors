namespace FishNet.Serializing
{
    /// <summary>
    /// How to pack data when using serialization.
    /// </summary>
    public enum AutoPackType : byte
    {
        /// <summary>
        /// Data will not be compressed.
        /// </summary>
        Unpacked = 0,
        /// <summary>
        /// Data will be compressed to use the least amount of data possible.
        /// </summary>
        Packed = 1,
        /// <summary>
        /// Data will be compressed but not as much as Packed.
        /// </summary>
        PackedLess = 2
    }
}