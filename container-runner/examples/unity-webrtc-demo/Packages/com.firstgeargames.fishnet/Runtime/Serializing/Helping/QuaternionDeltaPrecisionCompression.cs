using System;
using System.Collections.Generic;
using System.Linq;
using FishNet.Managing;
using UnityEngine;

namespace FishNet.Serializing.Helping
{
    [Flags]
    internal enum QuaternionDeltaPrecisionFlag : byte
    {
        Unset = 0,
        /* Its probably safe to discard '-IsNegative'
         * and replace with a single 'largest is negative'.
         * Doing this would still use the same amount of bytes
         * though, and would require a refactor on this and the delta
         * compression class. */
        NextAIsLarger = 1 << 0,
        NextBIsLarger = 1 << 1,
        NextCIsLarger = 1 << 2,
        NextDIsNegative = 1 << 3,
        LargestIsX = 1 << 4,
        LargestIsY = 1 << 5,
        LargestIsZ = 1 << 6,
        // This flag can be discarded via refactor if we need it later.
        LargestIsW = 1 << 7
    }

    internal static class QuaternionDeltaPrecisionFlagExtensions
    {
        /// <summary>
        /// Returns if whole contains part.
        /// </summary>
        internal static bool FastContains(this QuaternionDeltaPrecisionFlag whole, QuaternionDeltaPrecisionFlag part) => (whole & part) == part;
    }

    public static class QuaternionDeltaPrecisionCompression
    {
        /// <summary>
        /// Write a compressed a delta Quaternion using a variable precision.
        /// </summary>
        public static void Compress(Writer writer, Quaternion valueA, Quaternion valueB, float precision = 0.001f)
        {
            uint multiplier = (uint)Mathf.RoundToInt(1f / precision);

            // Position where the next byte is to be written.
            int startPosition = writer.Position;
            // Skip one byte so the flags can be inserted after everything else is writteh.
            writer.Skip(1);

            QuaternionDeltaPrecisionFlag flags = QuaternionDeltaPrecisionFlag.Unset;
            long largestUValue = -1;

            /* This becomes true if the largest difference is negative on valueB.
             * EG: if Y is the largest and value.Y is < 0f then largestIsNegative becomes true. */
            bool largestIsNegative = false;

            /* Set next is larger values, and output differneces. */
            bool xIsLarger = GetNextIsLarger(valueA.x, valueB.x, multiplier, out uint xDifference);
            UpdateLargestValues(xDifference, valueB.x, QuaternionDeltaPrecisionFlag.LargestIsX);

            bool yIsLarger = GetNextIsLarger(valueA.y, valueB.y, multiplier, out uint yDifference);
            UpdateLargestValues(yDifference, valueB.y, QuaternionDeltaPrecisionFlag.LargestIsY);

            bool zIsLarger = GetNextIsLarger(valueA.z, valueB.z, multiplier, out uint zDifference);
            UpdateLargestValues(zDifference, valueB.z, QuaternionDeltaPrecisionFlag.LargestIsZ);

            bool wIsLarger = GetNextIsLarger(valueA.w, valueB.w, multiplier, out uint wDifference);
            UpdateLargestValues(wDifference, valueB.w, QuaternionDeltaPrecisionFlag.LargestIsW);

            // If flags are unset something went wrong. This should never be possible.
            if (flags == QuaternionDeltaPrecisionFlag.Unset)
            {
                // Write that flags are unset and error.
                writer.InsertUInt8Unpacked((byte)flags, startPosition);
                writer.NetworkManager.LogError($"Flags should not be unset.");
                return;
            }

            // Updates largest values and flags.
            void UpdateLargestValues(uint checkedValue, float fValue, QuaternionDeltaPrecisionFlag newFlag)
            {
                if (checkedValue > largestUValue)
                {
                    largestUValue = checkedValue;
                    flags = newFlag;
                    largestIsNegative = fValue < 0f;
                }
            }

            /* Write all but largest. */

            // X is largest.
            if (flags == QuaternionDeltaPrecisionFlag.LargestIsX)
                WriteValues(yDifference, yIsLarger, zDifference, zIsLarger, wDifference, wIsLarger);
            // Y is largest.
            else if (flags == QuaternionDeltaPrecisionFlag.LargestIsY)
                WriteValues(xDifference, xIsLarger, zDifference, zIsLarger, wDifference, wIsLarger);
            // Z is largest.
            else if (flags == QuaternionDeltaPrecisionFlag.LargestIsZ)
                WriteValues(xDifference, xIsLarger, yDifference, yIsLarger, wDifference, wIsLarger);
            // W is largest.
            else if (flags == QuaternionDeltaPrecisionFlag.LargestIsW)
                WriteValues(xDifference, xIsLarger, yDifference, yIsLarger, zDifference, zIsLarger);

            /* This must be set after values are written since the enum
             * checks above use ==, rather than a bit comparer. */
            if (largestIsNegative)
                flags |= QuaternionDeltaPrecisionFlag.NextDIsNegative;

            void WriteValues(uint aValue, bool aIsLarger, uint bValue, bool bIsLarger, uint cValue, bool cIsLarger)
            {
                writer.WriteUnsignedPackedWhole(aValue);
                if (aIsLarger)
                    flags |= QuaternionDeltaPrecisionFlag.NextAIsLarger;

                writer.WriteUnsignedPackedWhole(bValue);
                if (bIsLarger)
                    flags |= QuaternionDeltaPrecisionFlag.NextBIsLarger;

                writer.WriteUnsignedPackedWhole(cValue);
                if (cIsLarger)
                    flags |= QuaternionDeltaPrecisionFlag.NextCIsLarger;
            }

            // Insert flags.
            writer.InsertUInt8Unpacked((byte)flags, startPosition);
        }

        /// <summary>
        /// Write a compressed a delta Quaternion using a variable precision.
        /// </summary>
        public static Quaternion Decompress(Reader reader, Quaternion valueA, float precision = 0.001f)
        {
            uint multiplier = (uint)Mathf.RoundToInt(1f / precision);

            QuaternionDeltaPrecisionFlag flags = (QuaternionDeltaPrecisionFlag)reader.ReadUInt8Unpacked();

            // Unset flags mean something went wrong in writing.
            if (flags == QuaternionDeltaPrecisionFlag.Unset)
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
            uint aWholeDifference = (uint)reader.ReadUnsignedPackedWhole();
            uint bWholeDifference = (uint)reader.ReadUnsignedPackedWhole();
            uint cWholeDifference = (uint)reader.ReadUnsignedPackedWhole();

            // Debug.Log($"Read  {aWholeDifference}, {bWholeDifference}, {cWholeDifference}. ValueA {valueA}");

            float aFloatDifference = (float)aWholeDifference / multiplier;
            float bFloatDifference = (float)bWholeDifference / multiplier;
            float cFloatDifference = (float)cWholeDifference / multiplier;

            // Invert differences as needed so they can all be added onto the previous value as negative or positive.
            if (!flags.FastContains(QuaternionDeltaPrecisionFlag.NextAIsLarger))
                aFloatDifference *= -1f;
            if (!flags.FastContains(QuaternionDeltaPrecisionFlag.NextBIsLarger))
                bFloatDifference *= -1f;
            if (!flags.FastContains(QuaternionDeltaPrecisionFlag.NextCIsLarger))
                cFloatDifference *= -1f;

            float nextA;
            float nextB;
            float nextC;

            /* Add onto the previous value. */
            if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsX))
            {
                nextA = valueA.y + aFloatDifference;
                nextB = valueA.z + bFloatDifference;
                nextC = valueA.w + cFloatDifference;
            }
            else if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsY))
            {
                nextA = valueA.x + aFloatDifference;
                nextB = valueA.z + bFloatDifference;
                nextC = valueA.w + cFloatDifference;
            }
            else if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsZ))
            {
                nextA = valueA.x + aFloatDifference;
                nextB = valueA.y + bFloatDifference;
                nextC = valueA.w + cFloatDifference;
            }
            /* We do not really need the 'largest is W' since we know if
             * the other 3 are not the largest, then the remaining must be.
             * We have the available packing to use though, so use them
             * for now. */
            else if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsW))
            {
                nextA = valueA.x + aFloatDifference;
                nextB = valueA.y + bFloatDifference;
                nextC = valueA.z + cFloatDifference;
            }
            else
            {
                reader.NetworkManager.LogError($"Largest axes was not handled. Flags {flags}.");
                return default;
            }

            float abcMagnitude = GetMagnitude(nextA, nextB, nextC);

            float nextD = 1f - abcMagnitude;
            /* NextD should always be positive. But depending on precision
             * the calculated result could be negative due to missing decimals.
             * When negative make positive so nextD will normalize properly. */
            if (nextD < 0f)
                nextD *= -1f;

            nextD = (float)Math.Sqrt(nextD);

            // Get magnitude of all values.
            static float GetMagnitude(float a, float b, float c, float d = 0f) => a * a + b * b + c * c + d * d;

            if (nextD >= 0f && flags.FastContains(QuaternionDeltaPrecisionFlag.NextDIsNegative))
                nextD *= -1f;

            if (!TryNormalize())
                return default;

            // Normalizes next values.
            bool TryNormalize()
            {
                float magnitude = (float)Math.Sqrt(GetMagnitude(nextA, nextB, nextC, nextD));
                if (magnitude < float.Epsilon)
                {
                    reader.NetworkManager.LogError($"Magnitude cannot be normalized.");
                    return false;
                }

                nextA /= magnitude;
                nextB /= magnitude;
                nextC /= magnitude;
                nextD /= magnitude;

                return true;
            }

            /* Add onto the previous value. */
            if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsX))
                return new(nextD, nextA, nextB, nextC);
            if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsY))
                return new(nextA, nextD, nextB, nextC);
            if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsZ))
                return new(nextA, nextB, nextD, nextC);
            if (flags.FastContains(QuaternionDeltaPrecisionFlag.LargestIsW))
                return new(nextA, nextB, nextC, nextD);

            reader.NetworkManager.LogError($"Unhandled Largest flag. Received flags are {flags}.");

            return default;
        }

        /// <summary>
        /// Returns if the next value is larger than the previous, and returns unsigned result with multiplier applied.
        /// </summary>
        private static bool GetNextIsLarger(float a, float b, uint lMultiplier, out uint multipliedUResult)
        {
            // Set is b is larger.
            bool bIsLarger = b > a;

            // Get multiplied u value.
            float value = bIsLarger ? b - a : a - b;
            multipliedUResult = (uint)Mathf.RoundToInt(value * lMultiplier);

            return bIsLarger;
        }
    }
}