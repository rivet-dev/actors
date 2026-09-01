#if UNITYMATHEMATICS
using System.Runtime.CompilerServices;
using Unity.Mathematics;

namespace FishNet.Serializing
{
    public partial class Writer
    {
        public void Writehalf(half value)
        {
            WriteUInt16(value.value);
        }

        public void Writehalf2(half2 value)
        {
            WriteUInt16(value.x.value);
            WriteUInt16(value.y.value);
        }

        public void Writehalf3(half3 value)
        {
            WriteUInt16(value.x.value);
            WriteUInt16(value.y.value);
            WriteUInt16(value.z.value);
        }

        public void Writehalf4(half4 value)
        {
            WriteUInt16(value.x.value);
            WriteUInt16(value.y.value);
            WriteUInt16(value.z.value);
            WriteUInt16(value.w.value);
        }
    }

    public partial class Reader
    {
        public half Readhalf()
        {
            return new() { value = ReadUInt16() };
        }

        public half2 Readhalf2()
        {
            half2 h = default;

            h.x.value = ReadUInt16();
            h.y.value = ReadUInt16();

            return h;
        }

        public half3 Readhalf3()
        {
            half3 h = default;

            h.x.value = ReadUInt16();
            h.y.value = ReadUInt16();
            h.z.value = ReadUInt16();

            return h;
        }

        public half4 Readhalf4()
        {
            half4 h = default;

            h.x.value = ReadUInt16();
            h.y.value = ReadUInt16();
            h.z.value = ReadUInt16();
            h.w.value = ReadUInt16();

            return h;
        }
    }
}
#endif