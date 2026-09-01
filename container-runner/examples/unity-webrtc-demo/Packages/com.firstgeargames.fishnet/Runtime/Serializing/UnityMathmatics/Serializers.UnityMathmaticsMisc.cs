#if UNITYMATHEMATICS
using System.Runtime.CompilerServices;
using Unity.Mathematics;
using Unity.Mathematics.Geometry;

namespace FishNet.Serializing
{
    public partial class Writer
    {
        public void Writequaternion(quaternion value)
        {
            Writefloat4(value.value);
        }

        public void Writerandom(Unity.Mathematics.Random random)
        {
            WriteUInt32(random.state);
        }

        public void WriteRigidTransform(RigidTransform value)
        {
            Writequaternion(value.rot);
            Writefloat3(value.pos);
        }
#if UNITYMATHEMATICS_131
        public void WriteAffineTransform(AffineTransform value)
        {
            Writefloat3x3(value.rs);
            Writefloat3(value.t);
        }
#endif
#if UNITYMATHEMATICS_132
        public void ReadMinMaxAABB(MinMaxAABB minMaxAABB)
        {
            Writefloat3(minMaxAABB.Min);
            Writefloat3(minMaxAABB.Max);
        }
#endif
    }

    public partial class Reader
    {
        public quaternion Readquaternion()
        {
            return new(Readfloat4());
        }

        public Random Readrandom()
        {
            return new() { state = ReadUInt32() };
        }

        public RigidTransform ReadRigidTransform()
        {
            return new()
            {
                rot = Readquaternion(),
                pos = Readfloat3(),
            };
        }

#if UNITYMATHEMATICS_131
        public AffineTransform ReadAffineTransform()
        {
            return new()
            {
                rs = Readfloat3x3(),
                t = Readfloat3(),
            };
        }
#endif
#if UNITYMATHEMATICS_132
        public MinMaxAABB ReadMinMaxAABB()
        {
            return new()
            {
                Min = Readfloat3(),
                Max = Readfloat3()
            };
        }
#endif
    }
}
#endif