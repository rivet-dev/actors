using System;
using UnityEngine;

namespace FishNet.Serializing.Helping
{
    public static class Quaternion32Compression
    {
        private const float Maximum = +1.0f / 1.414214f;
        private const int BitsPerAxis = 10;
        private const int LargestComponentShift = BitsPerAxis * 3;
        private const int AShift = BitsPerAxis * 2;
        private const int BShift = BitsPerAxis * 1;
        private const int IntScale = (1 << (BitsPerAxis - 1)) - 1;
        private const int IntMask = (1 << BitsPerAxis) - 1;

        /// <summary>
        /// </summary>
        /// <param name = "writer"></param>
        /// <param name = "quaternion"></param>
        /// <param name = "axesFlippingEnabled">True to flip the smaller values when the largest axes is negative. Doing this saves a byte but the rotation numeric values will be reversed when decompressed.</param>
        public static void Compress(Writer writer, Quaternion quaternion, bool axesFlippingEnabled = true)
        {
            const float precision = 0.00098f;

            float absX = Mathf.Abs(quaternion.x);
            float absY = Mathf.Abs(quaternion.y);
            float absZ = Mathf.Abs(quaternion.z);
            float absW = Mathf.Abs(quaternion.w);

            ComponentType largestComponent = ComponentType.X;
            float largestAbs = absX;
            float largest = quaternion.x;

            if (absY > largestAbs)
            {
                largestAbs = absY;
                largestComponent = ComponentType.Y;
                largest = quaternion.y;
            }
            if (absZ > largestAbs)
            {
                largestAbs = absZ;
                largestComponent = ComponentType.Z;
                largest = quaternion.z;
            }
            if (absW > largestAbs)
            {
                largestComponent = ComponentType.W;
                largest = quaternion.w;
            }

            bool largestIsNegative = largest < 0;

            // If not flipping axes and any values are less than precision then 0 them out.
            if (!axesFlippingEnabled)
            {
                if (absX < precision)
                    quaternion.x = 0f;
                if (absY < precision)
                    quaternion.y = 0f;
                if (absZ < precision)
                    quaternion.z = 0f;
                if (absW < precision)
                    quaternion.w = 0f;
            }

            float a = 0;
            float b = 0;
            float c = 0;
            switch (largestComponent)
            {
                case ComponentType.X:
                    a = quaternion.y;
                    b = quaternion.z;
                    c = quaternion.w;
                    break;
                case ComponentType.Y:
                    a = quaternion.x;
                    b = quaternion.z;
                    c = quaternion.w;
                    break;
                case ComponentType.Z:
                    a = quaternion.x;
                    b = quaternion.y;
                    c = quaternion.w;
                    break;
                case ComponentType.W:
                    a = quaternion.x;
                    b = quaternion.y;
                    c = quaternion.z;
                    break;
            }

            // If it's okay to flip when largest is negative.
            if (largestIsNegative && axesFlippingEnabled)
            {
                a = -a;
                b = -b;
                c = -c;
            }

            uint integerA = ScaleToUint(a);
            uint integerB = ScaleToUint(b);
            uint integerC = ScaleToUint(c);

            if (!axesFlippingEnabled)
                writer.WriteBoolean(largest < 0f);

            uint result = ((uint)largestComponent << LargestComponentShift) | (integerA << AShift) | (integerB << BShift) | integerC;
            writer.WriteUInt32Unpacked(result);
        }

        private static uint ScaleToUint(float v)
        {
            float normalized = v / Maximum;
            return (uint)Mathf.RoundToInt(normalized * IntScale) & IntMask;
        }

        private static float ScaleToFloat(uint v)
        {
            float unscaled = v * Maximum / IntScale;

            if (unscaled > Maximum)
                unscaled -= Maximum * 2;
            return unscaled;
        }

        /// <summary>
        /// </summary>
        /// <param name = "reader"></param>
        /// <param name = "axesFlippingEnabled">True if the smaller values were flipped during compression when the largest axes was negative.</param>
        /// <returns></returns>
        public static Quaternion Decompress(Reader reader, bool axesFlippingEnabled = true)
        {
            bool largestIsNegative = axesFlippingEnabled ? false : reader.ReadBoolean();
            uint compressed = reader.ReadUInt32Unpacked();

            ComponentType largestComponentType = (ComponentType)(compressed >> LargestComponentShift);
            uint integerA = (compressed >> AShift) & IntMask;
            uint integerB = (compressed >> BShift) & IntMask;
            uint integerC = compressed & IntMask;

            float a = ScaleToFloat(integerA);
            float b = ScaleToFloat(integerB);
            float c = ScaleToFloat(integerC);

            Quaternion rotation;
            switch (largestComponentType)
            {
                case ComponentType.X:
                    // (?) y z w
                    rotation.y = a;
                    rotation.z = b;
                    rotation.w = c;
                    rotation.x = Mathf.Sqrt(1 - rotation.y * rotation.y - rotation.z * rotation.z - rotation.w * rotation.w);

                    if (largestIsNegative)
                        rotation.x *= -1f;
                    break;
                case ComponentType.Y:
                    // x (?) z w
                    rotation.x = a;
                    rotation.z = b;
                    rotation.w = c;
                    rotation.y = Mathf.Sqrt(1 - rotation.x * rotation.x - rotation.z * rotation.z - rotation.w * rotation.w);

                    if (largestIsNegative)
                        rotation.y *= -1f;
                    break;
                case ComponentType.Z:
                    // x y (?) w
                    rotation.x = a;
                    rotation.y = b;
                    rotation.w = c;
                    rotation.z = Mathf.Sqrt(1 - rotation.x * rotation.x - rotation.y * rotation.y - rotation.w * rotation.w);

                    if (largestIsNegative)
                        rotation.z *= -1f;
                    break;
                case ComponentType.W:
                    // x y z (?)
                    rotation.x = a;
                    rotation.y = b;
                    rotation.z = c;
                    rotation.w = Mathf.Sqrt(1 - rotation.x * rotation.x - rotation.y * rotation.y - rotation.z * rotation.z);

                    if (largestIsNegative)
                        rotation.w *= -1f;
                    break;
                default:
                    // Should never happen!
                    throw new ArgumentOutOfRangeException("Unknown rotation component type: " + largestComponentType);
            }

            return rotation;
        }
    }
}