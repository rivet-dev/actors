#if UNITYMATHEMATICS
using System.Runtime.CompilerServices;
using Unity.Mathematics;

namespace FishNet.Serializing
{
    public partial class Writer
    {
        public void Writeint2(int2 value)
        {
            WriteInt32(value.x);
            WriteInt32(value.y);
        }

        public void Writeint3(int3 value)
        {
            WriteInt32(value.x);
            WriteInt32(value.y);
            WriteInt32(value.z);
        }

        public void Writeint4(int4 value)
        {
            WriteInt32(value.x);
            WriteInt32(value.y);
            WriteInt32(value.z);
            WriteInt32(value.w);
        }

        public void Writeint2x2(int2x2 value)
        {
            Writeint2(value.c0);
            Writeint2(value.c1);
        }

        public void Writeint2x3(int2x3 value)
        {
            Writeint2(value.c0);
            Writeint2(value.c1);
            Writeint2(value.c2);
        }

        public void Writeint2x4(int2x4 value)
        {
            Writeint2(value.c0);
            Writeint2(value.c1);
            Writeint2(value.c2);
            Writeint2(value.c3);
        }

        public void Writeint3x2(int3x2 value)
        {
            Writeint3(value.c0);
            Writeint3(value.c1);
        }

        public void Writeint3x3(int3x3 value)
        {
            Writeint3(value.c0);
            Writeint3(value.c1);
            Writeint3(value.c2);
        }

        public void Writeint3x4(int3x4 value)
        {
            Writeint3(value.c0);
            Writeint3(value.c1);
            Writeint3(value.c2);
            Writeint3(value.c3);
        }

        public void Writeint4x2(int4x2 value)
        {
            Writeint4(value.c0);
            Writeint4(value.c1);
        }

        public void Writeint4x3(int4x3 value)
        {
            Writeint4(value.c0);
            Writeint4(value.c1);
            Writeint4(value.c2);
        }

        public void Writeint4x4(int4x4 value)
        {
            Writeint4(value.c0);
            Writeint4(value.c1);
            Writeint4(value.c2);
            Writeint4(value.c3);
        }
    }

    public partial class Reader
    {
        public int2 Readint2()
        {
            return new()
            {
                x = ReadInt32(),
                y = ReadInt32()
            };
        }

        public int3 Readint3()
        {
            return new()
            {
                x = ReadInt32(),
                y = ReadInt32(),
                z = ReadInt32()
            };
        }

        public int4 Readint4()
        {
            return new()
            {
                x = ReadInt32(),
                y = ReadInt32(),
                z = ReadInt32(),
                w = ReadInt32()
            };
        }

        public int2x2 Readint2x2()
        {
            return new()
            {
                c0 = Readint2(),
                c1 = Readint2()
            };
        }

        public int2x3 Readint2x3()
        {
            return new()
            {
                c0 = Readint2(),
                c1 = Readint2(),
                c2 = Readint2()
            };
        }

        public int2x4 Readint2x4()
        {
            return new()
            {
                c0 = Readint2(),
                c1 = Readint2(),
                c2 = Readint2(),
                c3 = Readint2()
            };
        }

        public int3x2 Readint3x2()
        {
            return new()
            {
                c0 = Readint3(),
                c1 = Readint3()
            };
        }

        public int3x3 Readint3x3()
        {
            return new()
            {
                c0 = Readint3(),
                c1 = Readint3(),
                c2 = Readint3()
            };
        }

        public int3x4 Readint3x4()
        {
            return new()
            {
                c0 = Readint3(),
                c1 = Readint3(),
                c2 = Readint3(),
                c3 = Readint3()
            };
        }

        public int4x2 Readint4x2()
        {
            return new()
            {
                c0 = Readint4(),
                c1 = Readint4()
            };
        }

        public int4x3 Readint4x3()
        {
            return new()
            {
                c0 = Readint4(),
                c1 = Readint4(),
                c2 = Readint4()
            };
        }

        public int4x4 Readint4x4()
        {
            return new()
            {
                c0 = Readint4(),
                c1 = Readint4(),
                c2 = Readint4(),
                c3 = Readint4()
            };
        }
    }
}
#endif