using System;
using FishNet.CodeGenerating;
using System.Runtime.CompilerServices;
using FishNet.Managing;
using FishNet.Object;
using FishNet.Object.Prediction;
using FishNet.Serializing.Helping;
using UnityEngine;

namespace FishNet.Serializing
{
    public partial class Reader
    {
        internal double DOUBLE_ACCURACY => Writer.DOUBLE_ACCURACY;
        internal decimal DECIMAL_ACCURACY => Writer.DECIMAL_ACCURACY;

        #region Other.
        /// <summary>
        /// Reads a boolean.
        /// </summary>
        [DefaultDeltaReader]
        public bool ReadDeltaBoolean(bool valueA)
        {
            return !valueA;
        }
        #endregion

        #region Whole values.
        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public sbyte ReadDeltaInt8(sbyte valueA) => (sbyte)ReadDifference8_16_32(valueA);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public byte ReadDeltaUInt8(byte valueA) => (byte)ReadDifference8_16_32(valueA);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public short ReadDeltaInt16(short valueA) => (short)ReadDifference8_16_32(valueA);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public ushort ReadDeltaUInt16(ushort valueA) => (ushort)ReadDifference8_16_32(valueA);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public int ReadDeltaInt32(int valueA) => (int)ReadDifference8_16_32(valueA);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public uint ReadDeltaUInt32(uint valueA) => (uint)ReadDifference8_16_32(valueA);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public long ReadDeltaInt64(long valueA) => (long)ReadDeltaUInt64((ulong)valueA);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public ulong ReadDeltaUInt64(ulong valueA)
        {
            bool bLargerThanA = ReadBoolean();
            ulong diff = ReadUnsignedPackedWhole();

            return bLargerThanA ? valueA + diff : valueA - diff;
        }

        /// <summary>
        /// Returns a new result by reading and applying a difference to a value.
        /// </summary>
        [DefaultDeltaReader]
        private long ReadDifference8_16_32(long valueA)
        {
            long diff = ReadSignedPackedWhole();
            return valueA + diff;
        }
        #endregion

        #region Single.
        /// <summary>
        /// Reads a value.
        /// </summary>
        public float ReadDeltaSingle(UDeltaPrecisionType dpt, bool unsigned)
        {
            if (dpt.FastContains(UDeltaPrecisionType.UInt8))
            {
                if (unsigned)
                    return ReadUInt8Unpacked() / (float)DOUBLE_ACCURACY;
                else
                    return ReadInt8Unpacked() / (float)DOUBLE_ACCURACY;
            }
            else if (dpt.FastContains(UDeltaPrecisionType.UInt16))
            {
                if (unsigned)
                    return ReadUInt16Unpacked() / (float)DOUBLE_ACCURACY;
                else
                    return ReadInt16Unpacked() / (float)DOUBLE_ACCURACY;
            }
            // Everything else is unpacked.
            else
            {
                return ReadSingleUnpacked();
            }
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        public float ReadDeltaSingle(UDeltaPrecisionType dpt, float valueA, bool unsigned)
        {
            float diff = ReadDeltaSingle(dpt, unsigned);

            if (unsigned)
            {
                bool bLargerThanA = dpt.FastContains(UDeltaPrecisionType.NextValueIsLarger);
                return bLargerThanA ? valueA + diff : valueA - diff;
            }
            else
            {
                return valueA + diff;
            }
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        public float ReadDeltaSingle(float valueA)
        {
            const bool unsigned = false;
            UDeltaPrecisionType dpt = (UDeltaPrecisionType)ReadUInt8Unpacked();

            return ReadDeltaSingle(dpt, valueA, unsigned);
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public float ReadUDeltaSingle(float valueA)
        {
            const bool unsigned = true;
            UDeltaPrecisionType dpt = (UDeltaPrecisionType)ReadUInt8Unpacked();

            return ReadDeltaSingle(dpt, valueA, unsigned);
        }
        #endregion

        #region Double.
        /// <summary>
        /// Reads a value.
        /// </summary>
        public double ReadDeltaDouble(UDeltaPrecisionType dpt, bool unsigned)
        {
            if (dpt.FastContains(UDeltaPrecisionType.UInt8))
            {
                if (unsigned)
                    return ReadUInt8Unpacked() / DOUBLE_ACCURACY;
                else
                    return ReadInt8Unpacked() / DOUBLE_ACCURACY;
            }
            else if (dpt.FastContains(UDeltaPrecisionType.UInt16))
            {
                if (unsigned)
                    return ReadUInt16Unpacked() / DOUBLE_ACCURACY;
                else
                    return ReadInt16Unpacked() / DOUBLE_ACCURACY;
            }
            else if (dpt.FastContains(UDeltaPrecisionType.UInt32))
            {
                if (unsigned)
                    return ReadUInt32Unpacked() / DOUBLE_ACCURACY;
                else
                    return ReadInt32Unpacked() / DOUBLE_ACCURACY;
            }
            // Unpacked.
            else if (dpt.FastContains(UDeltaPrecisionType.Unset))
            {
                return ReadDoubleUnpacked();
            }
            else
            {
                NetworkManager.LogError($"Unhandled precision type of {dpt}.");
                return 0d;
            }
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        public double ReadDeltaDouble(UDeltaPrecisionType dpt, double valueA, bool unsigned)
        {
            double diff = ReadDeltaDouble(dpt, unsigned);
            // 8.

            if (unsigned)
            {
                bool bLargerThanA = dpt.FastContains(UDeltaPrecisionType.NextValueIsLarger);
                return bLargerThanA ? valueA + diff : valueA - diff;
            }
            else
            {
                return valueA + diff;
            }
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        public double ReadDeltaDouble(double valueA)
        {
            const bool unsigned = false;
            UDeltaPrecisionType dpt = (UDeltaPrecisionType)ReadUInt8Unpacked();

            return ReadDeltaDouble(dpt, valueA, unsigned);
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public double ReadUDeltaDouble(double valueA)
        {
            const bool unsigned = true;
            UDeltaPrecisionType dpt = (UDeltaPrecisionType)ReadUInt8Unpacked();

            return ReadDeltaDouble(dpt, valueA, unsigned);
        }
        #endregion

        #region Decimal.
        /// <summary>
        /// Reads a value.
        /// </summary>
        public decimal ReadDeltaDecimal(UDeltaPrecisionType dpt, bool unsigned)
        {
            if (dpt.FastContains(UDeltaPrecisionType.UInt8))
            {
                if (unsigned)
                    return ReadUInt8Unpacked() / DECIMAL_ACCURACY;
                else
                    return ReadInt8Unpacked() / DECIMAL_ACCURACY;
            }
            else if (dpt.FastContains(UDeltaPrecisionType.UInt16))
            {
                if (unsigned)
                    return ReadUInt16Unpacked() / DECIMAL_ACCURACY;
                else
                    return ReadInt16Unpacked() / DECIMAL_ACCURACY;
            }
            else if (dpt.FastContains(UDeltaPrecisionType.UInt32))
            {
                if (unsigned)
                    return ReadUInt32Unpacked() / DECIMAL_ACCURACY;
                else
                    return ReadInt32Unpacked() / DECIMAL_ACCURACY;
            }
            else if (dpt.FastContains(UDeltaPrecisionType.UInt64))
            {
                if (unsigned)
                    return ReadUInt64Unpacked() / DECIMAL_ACCURACY;
                else
                    return ReadInt64Unpacked() / DECIMAL_ACCURACY;
            }
            // Unpacked.
            else if (dpt.FastContains(UDeltaPrecisionType.Unset))
            {
                return ReadDecimalUnpacked();
            }
            else
            {
                NetworkManager.LogError($"Unhandled precision type of {dpt}.");
                return 0m;
            }
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        public decimal ReadDeltaDecimal(UDeltaPrecisionType dpt, decimal valueA, bool unsigned)
        {
            decimal diff = ReadDeltaDecimal(dpt, unsigned);

            if (unsigned)
            {
                bool bLargerThanA = dpt.FastContains(UDeltaPrecisionType.NextValueIsLarger);
                return bLargerThanA ? valueA + diff : valueA - diff;
            }
            else
            {
                return valueA + diff;
            }
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public decimal ReadDeltaDecimal(decimal valueA)
        {
            const bool unsigned = false;
            UDeltaPrecisionType dpt = (UDeltaPrecisionType)ReadUInt8Unpacked();

            return ReadDeltaDecimal(dpt, valueA, unsigned);
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public decimal ReadUDeltaDecimal(decimal valueA)
        {
            const bool unsigned = true;
            UDeltaPrecisionType dpt = (UDeltaPrecisionType)ReadUInt8Unpacked();

            return ReadDeltaDecimal(dpt, valueA, unsigned);
        }
        #endregion

        #region FishNet Types.
        /// <summary>
        /// Reads a delta value.
        /// </summary>
        /// <returns>True if written.</returns>
        [DefaultDeltaReader]
        public NetworkBehaviour WriteDeltaNetworkBehaviour(NetworkBehaviour valueA)
        {
            return ReadNetworkBehaviour();
        }
        #endregion

        #region Unity.
        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// (not really for Quaternion).
        /// </summary>
        [DefaultDeltaReader]
        public Quaternion ReadDeltaQuaternion(Quaternion valueA, float precision = Writer.QUATERNION_PRECISION) => QuaternionDeltaPrecisionCompression.Decompress(this, valueA, precision);

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public Vector2 ReadDeltaVector2(Vector2 valueA)
        {
            byte allFlags = ReadUInt8Unpacked();

            if ((allFlags & 1) == 1)
                valueA.x = ReadUDeltaSingle(valueA.x);
            if ((allFlags & 2) == 2)
                valueA.y = ReadUDeltaSingle(valueA.y);

            return valueA;
        }

        /// <summary>
        /// Reads a difference, appending it onto a value.
        /// </summary>
        [DefaultDeltaReader]
        public Vector3 ReadDeltaVector3(Vector3 valueA)
        {
            byte allFlags = ReadUInt8Unpacked();

            if ((allFlags & 1) == 1)
                valueA.x = ReadUDeltaSingle(valueA.x);
            if ((allFlags & 2) == 2)
                valueA.y = ReadUDeltaSingle(valueA.y);
            if ((allFlags & 4) == 4)
                valueA.z = ReadUDeltaSingle(valueA.z);

            return valueA;
        }
        #endregion

        #region Prediction.
        /// <summary>
        /// Reads a reconcile.
        /// </summary>
        internal T ReadDeltaReconcile<T>(T lastReconcile) => ReadDelta(lastReconcile);

        /// <summary>
        /// Reads a replicate.
        /// </summary>
        internal int ReadDeltaReplicate<T>(T lastReadReplicate, ref T[] collection, uint tick) where T : IReplicateData
        {
            int startRemaining = Remaining;

            // Number of entries written.
            int count = (int)ReadUInt8Unpacked();
            if (collection == null || collection.Length < count)
                collection = new T[count];

            /* Subtract count total minus 1
             * from starting tick. This sets the tick to what the first entry would be.
             * EG packet came in as tick 100, so that was passed as tick.
             * if there are 3 replicates then 2 would be subtracted (count - 1).
             * The new tick would be 98.
             * Ticks would be assigned to read values from oldest to
             * newest as 98, 99, 100. Which is the correct result. In order for this to
             * work properly past replicates cannot skip ticks. This will be ensured
             * in another part of the code. */
            tick -= (uint)(count - 1);

            uint lastReadTick = lastReadReplicate.GetTick();

            T prev = lastReadReplicate;
            for (int i = 0; i < count; i++)
            {
                // Tick read is for.
                uint readTick = tick + (uint)i;
                /* If readTick is equal or lesser than lastReadReplicate
                 * then there is no reason to process the data other than getting
                 * it out of the reader. */
                if (readTick <= lastReadTick)
                {
                    ReadDelta(prev);
                }
                else
                {
                    T value = ReadDelta(prev);
                    // Apply tick.
                    value.SetTick(readTick);
                    // Assign to collection.
                    collection[i] = value;
                    // Update previous.
                    prev = value;
                }
            }

            return count;
        }
        #endregion

        #region Generic.
        /// <summary>
        /// Reads a delta of any time.
        /// </summary>
        public T ReadDelta<T>(T prev)
        {
            Func<Reader, T, T> del = GenericDeltaReader<T>.Read;

            if (del == null)
            {
                NetworkManager.LogError($"Read delta method not found for {typeof(T).FullName}. Use a supported type or create a custom serializer.");
                return default;
            }
            else
            {
                return del.Invoke(this, prev);
            }
        }
        #endregion
    }
}