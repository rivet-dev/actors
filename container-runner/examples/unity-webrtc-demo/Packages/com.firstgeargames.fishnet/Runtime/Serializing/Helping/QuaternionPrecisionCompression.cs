using System;
using FishNet.Managing;
using UnityEngine;

namespace FishNet.Serializing.Helping
{
    [Flags]
    internal enum QuaternionPrecisionFlag : byte
    {
        Unset = 0,
        /* Its probably safe to discard '-IsNegative'
         * and replace with a single 'largest is negative'.
         * Doing this would still use the same amount of bytes
         * though, and would require a refactor on this and the delta
         * compression class. */
        AIsNegative = 1 << 0,
        BIsNegative = 1 << 1,
        CIsNegative = 1 << 2,
        DIsNegative = 1 << 3,
        LargestIsX = 1 << 4,
        LargestIsY = 1 << 5,
        LargestIsZ = 1 << 6,
        // This flag can be discarded via refactor if we need it later.
        LargestIsW = 1 << 7
    }

    internal static class QuaternionPrecisionFlagExtensions
    {
        /// <summary>
        /// Returns if whole contains part.
        /// </summary>
        internal static bool FastContains(this QuaternionPrecisionFlag whole, QuaternionPrecisionFlag part) => (whole & part) == part;
    }

    public static class QuaternionPrecisionCompression
    {
        /// <summary>
        /// Write a compressed a delta Quaternion using a variable precision.
        /// </summary>
        public static void Compress(Writer writer, Quaternion value, float precision = 0.001f)
        {
            /* When using 0.001f or less accurate precision use the classic
             * compression. This saves about a byte by send. */
            if (precision >= 0.001f)
            {
                Quaternion32Compression.Compress(writer, value, axesFlippingEnabled: false);
                return;
            }

            // Position where the next byte is to be written.
            int startPosition = writer.Position;

            // Skip one byte so the flags can be inserted after everything else is writteh.
            writer.Skip(1);

            QuaternionPrecisionFlag flags = QuaternionPrecisionFlag.Unset;
            float largestAxesValue = float.MinValue;

            // Find out which value is the largest.
            UpdateLargestValues(Math.Abs(value.x), QuaternionPrecisionFlag.LargestIsX);
            UpdateLargestValues(Math.Abs(value.y), QuaternionPrecisionFlag.LargestIsY);
            UpdateLargestValues(Math.Abs(value.z), QuaternionPrecisionFlag.LargestIsZ);
            UpdateLargestValues(Math.Abs(value.w), QuaternionPrecisionFlag.LargestIsW);

            // Updates largest values and flags.
            void UpdateLargestValues(float checkedValue, QuaternionPrecisionFlag newFlag)
            {
                if (checkedValue > largestAxesValue)
                {
                    largestAxesValue = checkedValue;
                    flags = newFlag;
                }
            }

            /* Write all but largest. */

            // X is largest.
            if (flags == QuaternionPrecisionFlag.LargestIsX)
                WriteValuesAndSetPositives(value.y, value.z, value.w, value.x);
            // Y is largest.
            else if (flags == QuaternionPrecisionFlag.LargestIsY)
                WriteValuesAndSetPositives(value.x, value.z, value.w, value.y);
            // Z is largest.
            else if (flags == QuaternionPrecisionFlag.LargestIsZ)
                WriteValuesAndSetPositives(value.x, value.y, value.w, value.z);
            // W is largest.
            else if (flags == QuaternionPrecisionFlag.LargestIsW)
                WriteValuesAndSetPositives(value.x, value.y, value.z, value.w);

            void WriteValuesAndSetPositives(float aValue, float bValue, float cValue, float largestAxes)
            {
                uint multiplier = (uint)Mathf.RoundToInt(1f / precision);

                uint aUint = (uint)Mathf.RoundToInt(Math.Abs(aValue) * multiplier);
                uint bUint = (uint)Mathf.RoundToInt(Math.Abs(bValue) * multiplier);
                uint cUint = (uint)Mathf.RoundToInt(Math.Abs(cValue) * multiplier);

                writer.WriteUnsignedPackedWhole(aUint);
                writer.WriteUnsignedPackedWhole(bUint);
                writer.WriteUnsignedPackedWhole(cUint);

                /* Update sign on values. */
                if (aValue < 0f)
                    flags |= QuaternionPrecisionFlag.AIsNegative;
                if (bValue < 0f)
                    flags |= QuaternionPrecisionFlag.BIsNegative;
                if (cValue <= 0f)
                    flags |= QuaternionPrecisionFlag.CIsNegative;
                if (largestAxes < 0f)
                    flags |= QuaternionPrecisionFlag.DIsNegative;
            }

            // Insert flags.
            writer.InsertUInt8Unpacked((byte)flags, startPosition);
        }

        /// <summary>
        /// Write a compressed a delta Quaternion using a variable precision.
        /// </summary>
        public static Quaternion Decompress(Reader reader, float precision = 0.001f)
        {
            /* When using 0.001f or less accurate precision use the classic
             * compression. This saves about a byte by send. */
            if (precision >= 0.001f)
                return Quaternion32Compression.Decompress(reader, axesFlippingEnabled: false);

            uint multiplier = (uint)Mathf.RoundToInt(1f / precision);

            QuaternionPrecisionFlag flags = (QuaternionPrecisionFlag)reader.ReadUInt8Unpacked();

            // Unset flags mean something went wrong in writing.
            if (flags == QuaternionPrecisionFlag.Unset)
            {
                reader.NetworkManager.LogError($"Unset flags were returned.");
                return default;
            }

            /* These values will be in order of X Y Z W.
             * Whichever value is the highest will be left out.
             *
             * EG: if Y was the highest then the following will be true...
             * a = X
             * b = Z
             * c = W */
            float aValue = (float)reader.ReadUnsignedPackedWhole() / (float)multiplier;
            float bValue = (float)reader.ReadUnsignedPackedWhole() / (float)multiplier;
            float cValue = (float)reader.ReadUnsignedPackedWhole() / (float)multiplier;

            // Make values negative if needed.
            if (flags.FastContains(QuaternionPrecisionFlag.AIsNegative))
                aValue *= -1f;
            if (flags.FastContains(QuaternionPrecisionFlag.BIsNegative))
                bValue *= -1f;
            if (flags.FastContains(QuaternionPrecisionFlag.CIsNegative))
                cValue *= -1f;

            float abcMagnitude = GetMagnitude(aValue, bValue, cValue);

            float dValue = 1f - abcMagnitude;
            /* NextD should always be positive. But depending on precision
             * the calculated result could be negative due to missing decimals.
             * When negative make positive so dValue will normalize properly. */
            if (dValue < 0f)
                dValue *= -1f;

            dValue = (float)Math.Sqrt(dValue);

            // Get magnitude of all values.
            static float GetMagnitude(float a, float b, float c, float d = 0f) => a * a + b * b + c * c + d * d;

            if (dValue >= 0f && flags.FastContains(QuaternionPrecisionFlag.DIsNegative))
                dValue *= -1f;

            if (!TryNormalize())
                return default;

            // Normalizes next values.
            bool TryNormalize()
            {
                float magnitude = (float)Math.Sqrt(GetMagnitude(aValue, bValue, cValue, dValue));
                if (magnitude < float.Epsilon)
                {
                    reader.NetworkManager.LogError($"Magnitude cannot be normalized.");
                    return false;
                }

                aValue /= magnitude;
                bValue /= magnitude;
                cValue /= magnitude;
                dValue /= magnitude;

                return true;
            }

            /* Add onto the previous value. */
            if (flags.FastContains(QuaternionPrecisionFlag.LargestIsX))
                return new(dValue, aValue, bValue, cValue);
            if (flags.FastContains(QuaternionPrecisionFlag.LargestIsY))
                return new(aValue, dValue, bValue, cValue);
            if (flags.FastContains(QuaternionPrecisionFlag.LargestIsZ))
                return new(aValue, bValue, dValue, cValue);
            if (flags.FastContains(QuaternionPrecisionFlag.LargestIsW))
                return new(aValue, bValue, cValue, dValue);

            reader.NetworkManager.LogError($"Unhandled Largest flag. Received flags are {flags}.");

            return default;
        }
    }
}