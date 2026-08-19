#if UNITYMATHEMATICS
using System.Runtime.CompilerServices;
using Unity.Mathematics;

namespace FishNet.Serializing
{
    public partial class Writer
    {
        public void Writefloat2(float2 value)
        {
            WriteSingle(value.x);
            WriteSingle(value.y);
        }

        public void Writefloat3(float3 value)
        {
            WriteSingle(value.x);
            WriteSingle(value.y);
            WriteSingle(value.z);
        }

        public void Writefloat4(float4 value)
        {
            WriteSingle(value.x);
            WriteSingle(value.y);
            WriteSingle(value.z);
            WriteSingle(value.w);
        }

        public void Writefloat2x2(float2x2 value)
        {
            Writefloat2(value.c0);
            Writefloat2(value.c1);
        }

        public void Writefloat2x3(float2x3 value)
        {
            Writefloat2(value.c0);
            Writefloat2(value.c1);
            Writefloat2(value.c2);
        }

        public void Writefloat2x4(float2x4 value)
        {
            Writefloat2(value.c0);
            Writefloat2(value.c1);
            Writefloat2(value.c2);
            Writefloat2(value.c3);
        }

        public void Writefloat3x2(float3x2 value)
        {
            Writefloat3(value.c0);
            Writefloat3(value.c1);
        }

        public void Writefloat3x3(float3x3 value)
        {
            Writefloat3(value.c0);
            Writefloat3(value.c1);
            Writefloat3(value.c2);
        }

        public void Writefloat3x4(float3x4 value)
        {
            Writefloat3(value.c0);
            Writefloat3(value.c1);
            Writefloat3(value.c2);
            Writefloat3(value.c3);
        }

        public void Writefloat4x2(float4x2 value)
        {
            Writefloat4(value.c0);
            Writefloat4(value.c1);
        }

        public void Writefloat4x3(float4x3 value)
        {
            Writefloat4(value.c0);
            Writefloat4(value.c1);
            Writefloat4(value.c2);
        }

        public void Writefloat4x4(float4x4 value)
        {
            Writefloat4(value.c0);
            Writefloat4(value.c1);
            Writefloat4(value.c2);
            Writefloat4(value.c3);
        }
    }

    public partial class Reader
    {
        public float2 Readfloat2()
        {
            return new()
            {
                x = ReadSingle(),
                y = ReadSingle()
            };
        }

        public float3 Readfloat3()
        {
            return new()
            {
                x = ReadSingle(),
                y = ReadSingle(),
                z = ReadSingle()
            };
        }

        public float4 Readfloat4()
        {
            return new()
            {
                x = ReadSingle(),
                y = ReadSingle(),
                z = ReadSingle(),
                w = ReadSingle()
            };
        }

        public float2x2 Readfloat2x2()
        {
            return new()
            {
                c0 = Readfloat2(),
                c1 = Readfloat2()
            };
        }

        public float2x3 Readfloat2x3()
        {
            return new()
            {
                c0 = Readfloat2(),
                c1 = Readfloat2(),
                c2 = Readfloat2()
            };
        }

        public float2x4 Readfloat2x4()
        {
            return new()
            {
                c0 = Readfloat2(),
                c1 = Readfloat2(),
                c2 = Readfloat2(),
                c3 = Readfloat2()
            };
        }

        public float3x2 Readfloat3x2()
        {
            return new()
            {
                c0 = Readfloat3(),
                c1 = Readfloat3()
            };
        }

        public float3x3 Readfloat3x3()
        {
            return new()
            {
                c0 = Readfloat3(),
                c1 = Readfloat3(),
                c2 = Readfloat3()
            };
        }

        public float3x4 Readfloat3x4()
        {
            return new()
            {
                c0 = Readfloat3(),
                c1 = Readfloat3(),
                c2 = Readfloat3(),
                c3 = Readfloat3()
            };
        }

        public float4x2 Readfloat4x2()
        {
            return new()
            {
                c0 = Readfloat4(),
                c1 = Readfloat4()
            };
        }

        public float4x3 Readfloat4x3()
        {
            return new()
            {
                c0 = Readfloat4(),
                c1 = Readfloat4(),
                c2 = Readfloat4()
            };
        }

        public float4x4 Readfloat4x4()
        {
            return new()
            {
                c0 = Readfloat4(),
                c1 = Readfloat4(),
                c2 = Readfloat4(),
                c3 = Readfloat4()
            };
        }
    }
}
#endif