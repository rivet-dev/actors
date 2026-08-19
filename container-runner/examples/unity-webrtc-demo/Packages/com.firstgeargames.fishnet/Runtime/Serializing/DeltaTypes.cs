using System.Runtime.CompilerServices;

namespace FishNet.Serializing
{
    /// <summary>
    /// This is for internal use and may change at any time.
    /// </summary>
    [System.Flags]
    public enum DeltaVector2Type : byte
    {
        /// <summary>
        /// This is unused.
        /// </summary>
        Unset = 0,
        /// <summary>
        /// Contains X as 1 byte.
        /// </summary>
        XUInt8 = 1,
        /// <summary>
        /// Contains X as 2 bytes.
        /// </summary>
        XUInt16 = 2,
        /// <summary>
        /// Contains X as 4 bytes.
        /// </summary>
        XUInt32 = 4,
        /// <summary>
        /// Contains Z as 1 byte.
        /// </summary>
        YUInt8 = 8,
        /// <summary>
        /// Contains Z as 2 bytes.
        /// </summary>
        YUInt16 = 16,
        /// <summary>
        /// Contains Z as 4 bytes.
        /// </summary>
        YUInt32 = 32,
        /// <summary>
        /// Contains Y as 1 byte.
        /// </summary>
        XNextIsLarger = 64,
        /// <summary>
        /// Contains Y as 4 bytes.
        /// </summary>
        YNextIsLarger = 128
    }

    /// <summary>
    /// This is for internal use and may change at any time.
    /// </summary>
    [System.Flags]
    public enum DeltaVector3Type : ushort
    {
        /// <summary>
        /// This is unused.
        /// </summary>
        Unset = 0,
        /// <summary>
        /// Contains X as 1 byte.
        /// </summary>
        XInt8 = 1,
        /// <summary>
        /// Contains X as 2 bytes.
        /// </summary>
        XInt16 = 2,
        /// <summary>
        /// Contains X as 4 bytes.
        /// </summary>
        XInt32 = 4,
        /// <summary>
        /// Contains Z as 1 byte.
        /// </summary>
        ZInt8 = 8,
        /// <summary>
        /// Contains Z as 2 bytes.
        /// </summary>
        ZInt16 = 16,
        /// <summary>
        /// Contains Z as 4 bytes.
        /// </summary>
        ZInt32 = 32,
        /// <summary>
        /// Contains Y as 1 byte.
        /// </summary>
        YInt8 = 64,
        /// <summary>
        /// Contains Y as 2 bytes.
        /// </summary>
        YInt32 = 128
    }

    [System.Flags]
    internal enum DeltaWholeType : byte
    {
        /// <summary>
        /// Indicates there is no compression. This can also be used to initialize the enum.
        /// </summary>
        Unset = 0,
        /// <summary>
        /// Data is written as a byte.
        /// </summary>
        UInt8 = 1,
        /// <summary>
        /// Data is written as a ushort.
        /// </summary>
        UInt16 = 2,
        /// <summary>
        /// Data is written as a uint.
        /// </summary>
        UInt32 = 4,
        /// <summary>
        /// Data is written as a ulong.
        /// </summary>
        UInt64 = 8,
        /// <summary>
        /// data is written as two ulong.
        /// </summary>
        UInt128 = 16,
        /// <summary>
        /// When set this indicates the new value is larger than the previous.
        /// When not set, indicates new value is smaller than the previous.
        /// </summary>
        NextValueIsLarger = 32
    }

    /// <summary>
    /// This is for internal use and may change at any time.
    /// </summary>
    [System.Flags]
    public enum UDeltaPrecisionType : byte
    {
        /// <summary>
        /// Indicates there is no compression. This can also be used to initialize the enum.
        /// </summary>
        Unset = 0,
        /// <summary>
        /// Data is written as a byte.
        /// </summary>
        UInt8 = 1,
        /// <summary>
        /// Data is written as a ushort.
        /// </summary>
        UInt16 = 2,
        /// <summary>
        /// Data is written as a uint.
        /// </summary>
        UInt32 = 4,
        /// <summary>
        /// Data is written as a ulong.
        /// </summary>
        UInt64 = 8,
        /// <summary>
        /// data is written as two ulong.
        /// </summary>
        UInt128 = 16,
        /// <summary>
        /// When set this indicates the new value is larger than the previous.
        /// When not set, indicates new value is smaller than the previous.
        /// </summary>
        NextValueIsLarger = 128
    }

    /// <summary>
    /// This is for internal use and may change at any time.
    /// </summary>
    public static class DeltaTypeExtensions
    {
        public static bool FastContains(this UDeltaPrecisionType whole, UDeltaPrecisionType part) => (whole & part) == part;
        public static bool FastContains(this UDeltaPrecisionType whole, UDeltaPrecisionType part, int shift) => FastContains((int)whole, (int)part, shift);
        public static bool FastContains(this DeltaVector3Type whole, DeltaVector3Type part) => (whole & part) == part;
        public static bool FastContains(this DeltaVector3Type whole, DeltaVector3Type part, int shift) => FastContains((int)whole, (int)part, shift);
        public static bool FastContains(this DeltaVector2Type whole, DeltaVector2Type part) => (whole & part) == part;

        private static bool FastContains(int whole, int part, int shift)
        {
            int intPart = part >> shift;
            return (whole & intPart) == intPart;
        }
    }
}