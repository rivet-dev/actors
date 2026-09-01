#if UNITYMATHEMATICS
using Unity.Mathematics;

namespace FishNet.Serializing
{
    public partial class Writer
    {
        public void Writebool2(bool2 value)
        {
            byte b = 0;

            if (value.x)
                b |= 1;
            if (value.y)
                b |= 2;

            WriteUInt8Unpacked(b);
        }

        public void Writebool3(bool3 value)
        {
            byte b = 0;

            if (value.x)
                b |= 1;
            if (value.y)
                b |= 2;
            if (value.z)
                b |= 4;

            WriteUInt8Unpacked(b);
        }

        public void Writebool4(bool4 value)
        {
            byte b = 0;

            if (value.x)
                b |= 1;
            if (value.y)
                b |= 2;
            if (value.z)
                b |= 4;
            if (value.w)
                b |= 8;

            WriteUInt8Unpacked(b);
        }

        public void Writebool2x2(bool2x2 value)
        {
            byte b = 0;

            if (value.c0.x)
                b |= 1;
            if (value.c0.y)
                b |= 2;
            if (value.c1.x)
                b |= 4;
            if (value.c1.y)
                b |= 8;

            WriteUInt8Unpacked(b);
        }

        public void Writebool2x3(bool2x3 value)
        {
            byte b = 0;

            if (value.c0.x)
                b |= 1;
            if (value.c0.y)
                b |= 2;
            if (value.c1.x)
                b |= 4;
            if (value.c1.y)
                b |= 8;
            if (value.c2.x)
                b |= 16;
            if (value.c2.y)
                b |= 32;

            WriteUInt8Unpacked(b);
        }

        public void Writebool2x4(bool2x4 value)
        {
            byte b = 0;

            if (value.c0.x)
                b |= 1;
            if (value.c0.y)
                b |= 2;
            if (value.c1.x)
                b |= 4;
            if (value.c1.y)
                b |= 8;
            if (value.c2.x)
                b |= 16;
            if (value.c2.y)
                b |= 32;
            if (value.c3.x)
                b |= 64;
            if (value.c3.y)
                b |= 128;

            WriteUInt8Unpacked(b);
        }

        public void Writebool3x2(bool3x2 value)
        {
            byte b = 0;

            if (value.c0.x)
                b |= 1;
            if (value.c0.y)
                b |= 2;
            if (value.c0.z)
                b |= 4;
            if (value.c1.x)
                b |= 8;
            if (value.c1.y)
                b |= 16;
            if (value.c1.z)
                b |= 32;

            WriteUInt8Unpacked(b);
        }

        public void Writebool3x3(bool3x3 value)
        {
            ushort s = 0;

            if (value.c0.x)
                s |= 1;
            if (value.c0.y)
                s |= 2;
            if (value.c0.z)
                s |= 4;
            if (value.c1.x)
                s |= 8;
            if (value.c1.y)
                s |= 16;
            if (value.c1.z)
                s |= 32;
            if (value.c2.x)
                s |= 64;
            if (value.c2.y)
                s |= 128;
            if (value.c2.z)
                s |= 256;

            WriteUInt16(s);
        }

        public void Writebool3x4(bool3x4 value)
        {
            ushort s = 0;

            if (value.c0.x)
                s |= 1;
            if (value.c0.y)
                s |= 2;
            if (value.c0.z)
                s |= 4;
            if (value.c1.x)
                s |= 8;
            if (value.c1.y)
                s |= 16;
            if (value.c1.z)
                s |= 32;
            if (value.c2.x)
                s |= 64;
            if (value.c2.y)
                s |= 128;
            if (value.c2.z)
                s |= 256;
            if (value.c3.x)
                s |= 512;
            if (value.c3.y)
                s |= 1024;
            if (value.c3.z)
                s |= 2048;

            WriteUInt16(s);
        }

        public void Writebool4x2(bool4x2 value)
        {
            byte b = 0;

            if (value.c0.x)
                b |= 1;
            if (value.c0.y)
                b |= 2;
            if (value.c0.z)
                b |= 4;
            if (value.c0.w)
                b |= 8;
            if (value.c1.x)
                b |= 16;
            if (value.c1.y)
                b |= 32;
            if (value.c1.z)
                b |= 64;
            if (value.c1.w)
                b |= 128;

            WriteUInt8Unpacked(b);
        }

        public void Writebool4x3(bool4x3 value)
        {
            ushort s = 0;

            if (value.c0.x)
                s |= 1;
            if (value.c0.y)
                s |= 2;
            if (value.c0.z)
                s |= 4;
            if (value.c0.w)
                s |= 8;
            if (value.c1.x)
                s |= 16;
            if (value.c1.y)
                s |= 32;
            if (value.c1.z)
                s |= 64;
            if (value.c1.w)
                s |= 128;
            if (value.c2.x)
                s |= 256;
            if (value.c2.y)
                s |= 512;
            if (value.c2.z)
                s |= 1024;
            if (value.c2.w)
                s |= 2048;

            WriteUInt16(s);
        }

        public void Writebool4x4(bool4x4 value)
        {
            ushort s = 0;

            if (value.c0.x)
                s |= 1;
            if (value.c0.y)
                s |= 2;
            if (value.c0.z)
                s |= 4;
            if (value.c0.w)
                s |= 8;
            if (value.c1.x)
                s |= 16;
            if (value.c1.y)
                s |= 32;
            if (value.c1.z)
                s |= 64;
            if (value.c1.w)
                s |= 128;
            if (value.c2.x)
                s |= 256;
            if (value.c2.y)
                s |= 512;
            if (value.c2.z)
                s |= 1024;
            if (value.c2.w)
                s |= 2048;
            if (value.c3.x)
                s |= 4096;
            if (value.c3.y)
                s |= 8192;
            if (value.c3.z)
                s |= 16384;
            if (value.c3.w)
                s |= 32768;

            WriteUInt16(s);
        }
    }

    public partial class Reader
    {
        public bool2 Readbool2()
        {
            byte b = ReadUInt8Unpacked();

            return new() { x = (b & 1) != 0, y = (b & 2) != 0 };
        }

        public bool3 Readbool3()
        {
            byte b = ReadUInt8Unpacked();

            return new()
            {
                x = (b & 1) != 0,
                y = (b & 2) != 0,
                z = (b & 4) != 0
            };
        }

        public bool4 Readbool4()
        {
            byte b = ReadUInt8Unpacked();

            return new()
            {
                x = (b & 1) != 0,
                y = (b & 2) != 0,
                z = (b & 4) != 0,
                w = (b & 8) != 0
            };
        }

        public bool2x2 Readbool2x2()
        {
            byte b = ReadUInt8Unpacked();

            bool2x2 value = default;

            value.c0.x = (b & 1) != 0;
            value.c0.y = (b & 2) != 0;
            value.c1.x = (b & 4) != 0;
            value.c1.y = (b & 8) != 0;

            return value;
        }

        public bool2x3 Readbool2x3()
        {
            byte b = ReadUInt8Unpacked();

            bool2x3 value = default;

            value.c0.x = (b & 1) != 0;
            value.c0.y = (b & 2) != 0;
            value.c1.x = (b & 4) != 0;
            value.c1.y = (b & 8) != 0;
            value.c2.x = (b & 16) != 0;
            value.c2.y = (b & 32) != 0;

            return value;
        }

        public bool2x4 Readbool2x4()
        {
            byte b = ReadUInt8Unpacked();

            bool2x4 value = default;

            value.c0.x = (b & 1) != 0;
            value.c0.y = (b & 2) != 0;
            value.c1.x = (b & 4) != 0;
            value.c1.y = (b & 8) != 0;
            value.c2.x = (b & 16) != 0;
            value.c2.y = (b & 32) != 0;
            value.c3.x = (b & 64) != 0;
            value.c3.y = (b & 128) != 0;

            return value;
        }

        public bool3x2 Readbool3x2()
        {
            byte b = ReadUInt8Unpacked();

            bool3x2 value = default;

            value.c0.x = (b & 1) != 0;
            value.c0.y = (b & 2) != 0;
            value.c0.z = (b & 4) != 0;
            value.c1.x = (b & 8) != 0;
            value.c1.y = (b & 16) != 0;
            value.c1.z = (b & 32) != 0;

            return value;
        }

        public bool3x3 Readbool3x3()
        {
            ushort s = ReadUInt16();

            bool3x3 value = default;
            value.c0.x = (s & 1) != 0;
            value.c0.y = (s & 2) != 0;
            value.c0.z = (s & 4) != 0;
            value.c1.x = (s & 8) != 0;
            value.c1.y = (s & 16) != 0;
            value.c1.z = (s & 32) != 0;
            value.c2.x = (s & 64) != 0;
            value.c2.y = (s & 128) != 0;
            value.c2.z = (s & 256) != 0;

            return value;
        }

        public bool3x4 Readbool3x4()
        {
            ushort s = ReadUInt16();

            bool3x4 value = default;

            value.c0.x = (s & 1) != 0;
            value.c0.y = (s & 2) != 0;
            value.c0.z = (s & 4) != 0;
            value.c1.x = (s & 8) != 0;
            value.c1.y = (s & 16) != 0;
            value.c1.z = (s & 32) != 0;
            value.c2.x = (s & 64) != 0;
            value.c2.y = (s & 128) != 0;
            value.c2.z = (s & 256) != 0;
            value.c3.x = (s & 512) != 0;
            value.c3.y = (s & 1024) != 0;
            value.c3.z = (s & 2048) != 0;

            return value;
        }

        public bool4x2 Readbool4x2()
        {
            byte b = ReadUInt8Unpacked();

            bool4x2 value = default;

            value.c0.x = (b & 1) != 0;
            value.c0.y = (b & 2) != 0;
            value.c0.z = (b & 4) != 0;
            value.c0.w = (b & 8) != 0;
            value.c1.x = (b & 16) != 0;
            value.c1.y = (b & 32) != 0;
            value.c1.z = (b & 64) != 0;
            value.c1.w = (b & 128) != 0;

            return value;
        }

        public bool4x3 Readbool4x3()
        {
            ushort s = ReadUInt16();

            bool4x3 value = default;

            value.c0.x = (s & 1) != 0;
            value.c0.y = (s & 2) != 0;
            value.c0.z = (s & 4) != 0;
            value.c0.w = (s & 8) != 0;
            value.c1.x = (s & 16) != 0;
            value.c1.y = (s & 32) != 0;
            value.c1.z = (s & 64) != 0;
            value.c1.w = (s & 128) != 0;
            value.c2.x = (s & 256) != 0;
            value.c2.y = (s & 512) != 0;
            value.c2.z = (s & 1024) != 0;
            value.c2.w = (s & 2048) != 0;

            return value;
        }

        public bool4x4 Readbool4x4()
        {
            ushort s = ReadUInt16();

            bool4x4 value = default;

            value.c0.x = (s & 1) != 0;
            value.c0.y = (s & 2) != 0;
            value.c0.z = (s & 4) != 0;
            value.c0.w = (s & 8) != 0;
            value.c1.x = (s & 16) != 0;
            value.c1.y = (s & 32) != 0;
            value.c1.z = (s & 64) != 0;
            value.c1.w = (s & 128) != 0;
            value.c2.x = (s & 256) != 0;
            value.c2.y = (s & 512) != 0;
            value.c2.z = (s & 1024) != 0;
            value.c2.w = (s & 2048) != 0;
            value.c3.x = (s & 4096) != 0;
            value.c3.y = (s & 8192) != 0;
            value.c3.z = (s & 16384) != 0;
            value.c3.w = (s & 32768) != 0;

            return value;
        }
    }
}
#endif