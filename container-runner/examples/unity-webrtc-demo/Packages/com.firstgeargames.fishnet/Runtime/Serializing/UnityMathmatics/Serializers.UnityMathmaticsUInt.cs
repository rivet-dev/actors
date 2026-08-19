#if UNITYMATHEMATICS
using System.Runtime.CompilerServices;
using Unity.Mathematics;

namespace FishNet.Serializing
{
    public partial class Writer
    {
        public void Writeuint2(uint2 value)
        {
            WriteUInt32(value.x);
            WriteUInt32(value.y);
        }

        public void Writeuint3(uint3 value)
        {
            WriteUInt32(value.x);
            WriteUInt32(value.y);
            WriteUInt32(value.z);
        }

        public void Writeuint4(uint4 value)
        {
            WriteUInt32(value.x);
            WriteUInt32(value.y);
            WriteUInt32(value.z);
            WriteUInt32(value.w);
        }

        public void Writeuint2x2(uint2x2 value)
        {
            Writeuint2(value.c0);
            Writeuint2(value.c1);
        }

        public void Writeuint2x3(uint2x3 value)
        {
            Writeuint2(value.c0);
            Writeuint2(value.c1);
            Writeuint2(value.c2);
        }

        public void Writeuint2x4(uint2x4 value)
        {
            Writeuint2(value.c0);
            Writeuint2(value.c1);
            Writeuint2(value.c2);
            Writeuint2(value.c3);
        }

        public void Writeuint3x2(uint3x2 value)
        {
            Writeuint3(value.c0);
            Writeuint3(value.c1);
        }

        public void Writeuint3x3(uint3x3 value)
        {
            Writeuint3(value.c0);
            Writeuint3(value.c1);
            Writeuint3(value.c2);
        }

        public void Writeuint3x4(uint3x4 value)
        {
            Writeuint3(value.c0);
            Writeuint3(value.c1);
            Writeuint3(value.c2);
            Writeuint3(value.c3);
        }

        public void Writeuint4x2(uint4x2 value)
        {
            Writeuint4(value.c0);
            Writeuint4(value.c1);
        }

        public void Writeuint4x3(uint4x3 value)
        {
            Writeuint4(value.c0);
            Writeuint4(value.c1);
            Writeuint4(value.c2);
        }

        public void Writeuint4x4(uint4x4 value)
        {
            Writeuint4(value.c0);
            Writeuint4(value.c1);
            Writeuint4(value.c2);
            Writeuint4(value.c3);
        }
    }

    public partial class Reader
    {
        public uint2 Readuint2()
        {
            return new()
            {
                x = ReadUInt32(),
                y = ReadUInt32()
            };
        }

        public uint3 Readuint3()
        {
            return new()
            {
                x = ReadUInt32(),
                y = ReadUInt32(),
                z = ReadUInt32()
            };
        }

        public uint4 Readuint4()
        {
            return new()
            {
                x = ReadUInt32(),
                y = ReadUInt32(),
                z = ReadUInt32(),
                w = ReadUInt32()
            };
        }

        public uint2x2 Readuint2x2()
        {
            return new()
            {
                c0 = Readuint2(),
                c1 = Readuint2()
            };
        }

        public uint2x3 Readuint2x3()
        {
            return new()
            {
                c0 = Readuint2(),
                c1 = Readuint2(),
                c2 = Readuint2()
            };
        }

        public uint2x4 Readuint2x4()
        {
            return new()
            {
                c0 = Readuint2(),
                c1 = Readuint2(),
                c2 = Readuint2(),
                c3 = Readuint2()
            };
        }

        public uint3x2 Readuint3x2()
        {
            return new()
            {
                c0 = Readuint3(),
                c1 = Readuint3()
            };
        }

        public uint3x3 Readuint3x3()
        {
            return new()
            {
                c0 = Readuint3(),
                c1 = Readuint3(),
                c2 = Readuint3()
            };
        }

        public uint3x4 Readuint3x4()
        {
            return new()
            {
                c0 = Readuint3(),
                c1 = Readuint3(),
                c2 = Readuint3(),
                c3 = Readuint3()
            };
        }

        public uint4x2 Readuint4x2()
        {
            return new()
            {
                c0 = Readuint4(),
                c1 = Readuint4()
            };
        }

        public uint4x3 Readuint4x3()
        {
            return new()
            {
                c0 = Readuint4(),
                c1 = Readuint4(),
                c2 = Readuint4()
            };
        }

        public uint4x4 Readuint4x4()
        {
            return new()
            {
                c0 = Readuint4(),
                c1 = Readuint4(),
                c2 = Readuint4(),
                c3 = Readuint4()
            };
        }
    }
}
#endif