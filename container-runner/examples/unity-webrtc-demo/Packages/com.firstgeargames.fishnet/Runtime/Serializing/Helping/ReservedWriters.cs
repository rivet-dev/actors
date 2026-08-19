using System.Runtime.CompilerServices;
using FishNet.Managing;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Serializing.Helping
{
    /// <summary>
    /// Used to reserve bytes in a writer for length, then inserts length after data has been written.
    /// Reserved values are always written as unsigned.
    /// </summary>
    internal class ReservedLengthWriter : IResettable
    {
        private Writer _writer;
        private int _startPosition;
        private byte _reservedBytes;
        /// <summary>
        /// Number of bytes currently written.
        /// </summary>
        public int Length
        {
            get { return _writer == null ? 0 : _writer.Position - _startPosition; }
        }

        public void Initialize(Writer writer, byte reservedBytes)
        {
            _writer = writer;
            _reservedBytes = reservedBytes;
            writer.Skip(reservedBytes);
            _startPosition = writer.Position;
        }

        /// <summary>
        /// Writes the amount of data written to the reserved space.
        /// This also resets the state of this object.
        /// </summary>
        public void WriteLength()
        {
            WriteLength((uint)Length);

            ResetState();
        }

        /// <summary>
        /// Writes the amount of data written to the reserved space. If no data was written the reserved amount is removed.
        /// This also resets the state of this object.
        /// Returns if length was written.
        /// </summary>
        public bool WriteLengthOrRemove(uint written)
        {
            if (written == 0)
                _writer.Remove(_reservedBytes);
            else
                WriteLength(written);

            ResetState();

            return written > 0;
        }

        /// <summary>
        /// Writes the amount of data written to the reserved space. This overrides Length normally written.
        /// This also resets the state of this object.
        /// </summary>
        public void WriteLength(uint written)
        {
            switch (_reservedBytes)
            {
                case 1:
                    _writer.InsertUInt8Unpacked((byte)written, _startPosition - _reservedBytes);
                    break;
                case 2:
                    _writer.InsertUInt16Unpacked((ushort)written, _startPosition - _reservedBytes);
                    break;
                case 4:
                    _writer.InsertUInt32Unpacked((uint)written, _startPosition - _reservedBytes);
                    break;
                default:
                    NetworkManager nm = _writer == null ? null : _writer.NetworkManager;
                    nm.LogError($"Reserved bytes value of {_reservedBytes} is unhandled.");
                    break;
            }

            ResetState();
        }

        /// <summary>
        /// Writes the amount of data written to the reserved space. If no data was written the reserved amount is removed.
        /// This also resets the state of this object.
        /// </summary>
        public bool WriteLengthOrRemove()
        {
            // Insert written amount.
            int written = _writer.Position - _startPosition;

            if (written == 0)
                _writer.Remove(_reservedBytes);
            else
                WriteLength((uint)written);

            ResetState();

            return written > 0;
        }

        /// <summary>
        /// Returns a length read based on a reserved byte count.
        /// </summary>
        /// <param name = "resetPosition">True to reset to position before read.</param>
        public static uint ReadLength(PooledReader reader, byte reservedBytes, bool resetPosition = false)
        {
            uint result;
            switch (reservedBytes)
            {
                case 1:
                    result = reader.ReadUInt8Unpacked();
                    break;
                case 2:
                    result = reader.ReadUInt16Unpacked();
                    break;
                case 4:
                    result = reader.ReadUInt32Unpacked();
                    break;
                default:
                    NetworkManager nm = reader == null ? null : reader.NetworkManager;
                    nm.LogError($"Reserved bytes value of {reservedBytes} is unhandled.");
                    return 0;
            }

            if (resetPosition)
                reader.Position -= (int)result;

            return result;
        }

        public void ResetState()
        {
            _writer = null;
            _startPosition = 0;
            _reservedBytes = 0;
        }

        public void InitializeState() { }
    }

    internal static class ReservedWritersExtensions
    {
        /// <summary>
        /// Stores to a cache.
        /// </summary>
        public static void Store(this ReservedLengthWriter rlw) => ResettableObjectCaches<ReservedLengthWriter>.Store(rlw);

        /// <summary>
        /// Retrieves from a cache.
        /// </summary>
        /// <returns></returns>
        public static ReservedLengthWriter Retrieve() => ResettableObjectCaches<ReservedLengthWriter>.Retrieve();
    }
}