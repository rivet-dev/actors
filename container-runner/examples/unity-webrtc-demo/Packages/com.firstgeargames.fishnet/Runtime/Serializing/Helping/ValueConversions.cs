using System.Runtime.InteropServices;

namespace FishNet.Serializing.Helping
{
    [StructLayout(LayoutKind.Explicit)]
    internal struct UIntFloat
    {
        [FieldOffset(0)]
        public float FloatValue;
        [FieldOffset(0)]
        public uint UIntValue;
    }

    [StructLayout(LayoutKind.Explicit)]
    internal struct UIntDouble
    {
        [FieldOffset(0)]
        public double DoubleValue;
        [FieldOffset(0)]
        public ulong LongValue;
    }

    [StructLayout(LayoutKind.Explicit)]
    internal struct UIntDecimal
    {
        [FieldOffset(0)]
        public ulong LongValue1;
        [FieldOffset(8)]
        public ulong LongValue2;
        [FieldOffset(0)]
        public decimal DecimalValue;
    }
}