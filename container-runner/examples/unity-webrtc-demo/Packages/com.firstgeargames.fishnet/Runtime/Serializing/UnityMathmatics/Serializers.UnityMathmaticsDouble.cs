#if UNITYMATHEMATICS
using System.Runtime.CompilerServices;
using Unity.Mathematics;

namespace FishNet.Serializing
{
    public partial class Writer
    {
        public void Writedouble2(double2 value)
        {
            WriteDouble(value.x);
            WriteDouble(value.y);
        }

        public void Writedouble3(double3 value)
        {
            WriteDouble(value.x);
            WriteDouble(value.y);
            WriteDouble(value.z);
        }

        public void Writedouble4(double4 value)
        {
            WriteDouble(value.x);
            WriteDouble(value.y);
            WriteDouble(value.z);
            WriteDouble(value.w);
        }

        public void Writedouble2x2(double2x2 value)
        {
            Writedouble2(value.c0);
            Writedouble2(value.c1);
        }

        public void Writedouble2x3(double2x3 value)
        {
            Writedouble2(value.c0);
            Writedouble2(value.c1);
            Writedouble2(value.c2);
        }

        public void Writedouble2x4(double2x4 value)
        {
            Writedouble2(value.c0);
            Writedouble2(value.c1);
            Writedouble2(value.c2);
            Writedouble2(value.c3);
        }

        public void Writedouble3x2(double3x2 value)
        {
            Writedouble3(value.c0);
            Writedouble3(value.c1);
        }

        public void Writedouble4x2(double4x2 value)
        {
            Writedouble4(value.c0);
            Writedouble4(value.c1);
        }

        public void Writedouble3x4(double3x4 value)
        {
            Writedouble3(value.c0);
            Writedouble3(value.c1);
            Writedouble3(value.c2);
            Writedouble3(value.c3);
        }

        public void Writedouble4x3(double4x3 value)
        {
            Writedouble4(value.c0);
            Writedouble4(value.c1);
            Writedouble4(value.c2);
        }

        public void Writedouble3x3(double3x3 value)
        {
            Writedouble3(value.c0);
            Writedouble3(value.c1);
            Writedouble3(value.c2);
        }

        public void Writedouble4x4(double4x4 value)
        {
            Writedouble4(value.c0);
            Writedouble4(value.c1);
            Writedouble4(value.c2);
            Writedouble4(value.c3);
        }
    }

    public partial class Reader
    {
        public double2 Readdouble2()
        {
            return new()
            {
                x = ReadDouble(),
                y = ReadDouble()
            };
        }

        public double3 Readdouble3()
        {
            return new()
            {
                x = ReadDouble(),
                y = ReadDouble(),
                z = ReadDouble()
            };
        }

        public double4 Readdouble4()
        {
            return new()
            {
                x = ReadDouble(),
                y = ReadDouble(),
                z = ReadDouble(),
                w = ReadDouble()
            };
        }

        public double2x2 Readdouble2x2()
        {
            return new()
            {
                c0 = Readdouble2(),
                c1 = Readdouble2()
            };
        }

        public double2x3 Readdouble2x3()
        {
            return new()
            {
                c0 = Readdouble2(),
                c1 = Readdouble2(),
                c2 = Readdouble2()
            };
        }

        public double2x4 Readdouble2x4()
        {
            return new()
            {
                c0 = Readdouble2(),
                c1 = Readdouble2(),
                c2 = Readdouble2(),
                c3 = Readdouble2()
            };
        }

        public double3x2 Readdouble3x2()
        {
            return new()
            {
                c0 = Readdouble3(),
                c1 = Readdouble3()
            };
        }

        public double4x2 Readdouble4x2()
        {
            return new()
            {
                c0 = Readdouble4(),
                c1 = Readdouble4()
            };
        }

        public double3x4 Readdouble3x4()
        {
            return new()
            {
                c0 = Readdouble3(),
                c1 = Readdouble3(),
                c2 = Readdouble3(),
                c3 = Readdouble3()
            };
        }

        public double4x3 Readdouble4x3()
        {
            return new()
            {
                c0 = Readdouble4(),
                c1 = Readdouble4(),
                c2 = Readdouble4()
            };
        }

        public double3x3 Readdouble3x3()
        {
            return new()
            {
                c0 = Readdouble3(),
                c1 = Readdouble3(),
                c2 = Readdouble3()
            };
        }

        public double4x4 Readdouble4x4()
        {
            return new()
            {
                c0 = Readdouble4(),
                c1 = Readdouble4(),
                c2 = Readdouble4(),
                c3 = Readdouble4()
            };
        }
    }
}
#endif