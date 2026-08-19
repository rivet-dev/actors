using FishNet.CodeGenerating;
using FishNet.Connection;
using FishNet.Managing;
using FishNet.Object;
using FishNet.Object.Prediction;
using FishNet.Serializing.Helping;
using FishNet.Transporting;
using FishNet.Utility;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Text;
using GameKit.Dependencies.Utilities.Types;
using UnityEngine;

[assembly: InternalsVisibleTo(UtilityConstants.GENERATED_ASSEMBLY_NAME)]

namespace FishNet.Serializing
{
    /// <summary>
    /// Writes data to a buffer.
    /// </summary>
    public partial class Writer
    {
        #region Public.
        /// <summary>
        /// Capacity of the buffer.
        /// </summary>
        public int Capacity => _buffer.Length;
        /// <summary>
        /// Current write position.
        /// </summary>
        public int Position;
        /// <summary>
        /// Number of bytes writen to the buffer.
        /// </summary>
        public int Length;
        /// <summary>
        /// NetworkManager associated with this writer. May be null.
        /// </summary>
        public NetworkManager NetworkManager;
        #endregion
        
        #region Private.
        /// <summary>
        /// Buffer to prevent new allocations. This will grow as needed.
        /// </summary>
        private byte[] _buffer = new byte[64];
        /// <summary>
        /// A buffer convert Guid data.
        /// </summary>
        private static readonly byte[] _guidBuffer = new byte[16];
        #endregion

        #region Const.
        /// <summary>
        /// Replicate data is default of T.
        /// </summary>
        internal const byte REPLICATE_DEFAULT_BYTE = 0;
        /// <summary>
        /// Replicate data is the same as the previous.
        /// </summary>
        internal const byte REPLICATE_DUPLICATE_BYTE = 1;
        /// <summary>
        /// Replicate data is different from the previous.
        /// </summary>
        internal const byte REPLICATE_UNIQUE_BYTE = 2;
        /// <summary>
        /// Replicate data is repeating for every entry.
        /// </summary>
        internal const byte REPLICATE_REPEATING_BYTE = 3;
        /// <summary>
        /// All datas in the replicate are default.
        /// </summary>
        internal const byte REPLICATE_ALL_DEFAULT_BYTE = 4;
        /// <summary>
        /// Value used when a collection is unset, as in null.
        /// </summary>
        public const int UNSET_COLLECTION_SIZE_VALUE = -1;
        #endregion

        /// <summary>
        /// Outputs writer to string.
        /// </summary>
        /// <returns></returns>
        public override string ToString() => ToString(0, Length);

        /// <summary>
        /// Outputs writer to string starting at an index.
        /// </summary>
        /// <returns></returns>
        public string ToString(int offset, int length)
        {
            return $"Position: {Position:0000}, Length: {Length:0000}, Buffer: {BitConverter.ToString(_buffer, offset, length)}.";
        }

        [Obsolete("Use Clear(NetworkManager) instead.")]
        public void Reset(NetworkManager newManager = null) => Clear(newManager);

        /// <summary>
        /// Resets written data.
        /// </summary>
        public void Clear()
        {
            Length = 0;
            Position = 0;
        }

        /// <summary>
        /// Resets written data and sets the NetworkManager.
        /// </summary>
        public void Clear(NetworkManager newManager)
        {
            Clear();
            NetworkManager = newManager;
        }

        /// <summary>
        /// Ensures the buffer Capacity is of minimum count.
        /// </summary>
        /// <param name = "count"></param>
        public void EnsureBufferCapacity(int count)
        {
            if (Capacity < count)
                Array.Resize(ref _buffer, count);
        }

        /// <summary>
        /// Ensure a number of bytes to be available in the buffer from current position.
        /// </summary>
        /// <param name = "count"></param>
        public void EnsureBufferLength(int count)
        {
            if (Position + count > _buffer.Length)
            {
                int nextSize = _buffer.Length * 2 + count;
                Array.Resize(ref _buffer, nextSize);
            }
        }

        /// <summary>
        /// Returns the buffer. The returned value will be the full buffer, even if not all of it is used.
        /// </summary>
        /// <returns></returns>
        public byte[] GetBuffer()
        {
            return _buffer;
        }

        /// <summary>
        /// Returns the used portion of the buffer as an ArraySegment.
        /// </summary>
        /// <returns></returns>
        public ArraySegment<byte> GetArraySegment()
        {
            return new(_buffer, 0, Length);
        }

        /// <summary>
        /// Reserves a number of bytes from current position.
        /// </summary>
        /// <param name = "count"></param>
        [Obsolete("Use Skip.")]
        public void Reserve(int count) => Skip(count);

        /// <summary>
        /// Skips a number of bytes from current position.
        /// </summary>
        /// <param name = "count"></param>
        public void Skip(int count)
        {
            EnsureBufferLength(count);
            Position += count;
            Length = Math.Max(Length, Position);
        }

        /// <summary>
        /// Sets size variables back an amount.
        /// </summary>
        /// <param name = "count"></param>
        internal void Remove(int count)
        {
            Position -= count;
            Length -= count;
        }

        /// <summary>
        /// Sends a packetId.
        /// </summary>
        /// <param name = "pid"></param>
        internal void WritePacketIdUnpacked(PacketId pid)
        {
            WriteUInt16Unpacked((ushort)pid);
        }

        /// <summary>
        /// Inserts a packetId.
        /// </summary>
        internal void InsertPacketIdUnpacked(PacketId packetId, int index)
        {
            ushort pId = (ushort)packetId;
            InsertUInt16Unpacked(pId, index);
        }

        /// <summary>
        /// Inserts value at index within the buffer.
        /// This method does not perform error checks nor increases Length, Position.
        /// </summary>
        [Obsolete("Use InsertUInt8Unpacked.")]
        public void FastInsertUInt8Unpacked(byte value, int index) => InsertUInt8Unpacked(value, index);

        /// <summary>
        /// Inserts value at index within the buffer.
        /// This method does not perform error checks nor increases Length, Position.
        /// </summary>
        public void InsertUInt8Unpacked(byte value, int index)
        {
            _buffer[index] = value;
        }

        /// <summary>
        /// Inserts value at index within the buffer.
        /// This method does not perform error checks nor increases Length, Position.
        /// </summary>
        public void InsertUInt16Unpacked(ushort value, int index)
        {
            _buffer[index++] = (byte)value;
            _buffer[index] = (byte)(value >> 8);
        }

        /// <summary>
        /// Inserts value at index within the buffer.
        /// This method does not perform error checks nor increases Length, Position.
        /// </summary>
        public void InsertInt32Unpacked(int value, int index) => InsertUInt32Unpacked((uint)value, index);

        /// <summary>
        /// Inserts value at index within the buffer.
        /// This method does not perform error checks nor increases Length, Position.
        /// </summary>
        public void InsertUInt32Unpacked(uint value, int index)
        {
            _buffer[index++] = (byte)value;
            _buffer[index++] = (byte)(value >> 8);
            _buffer[index++] = (byte)(value >> 16);
            _buffer[index] = (byte)(value >> 24);
        }

        [Obsolete("Use WriteUInt8Unpacked.")]
        public void WriteByte(byte value) => WriteUInt8Unpacked(value);

        /// <summary>
        /// Writes a byte.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteUInt8Unpacked(byte value)
        {
            EnsureBufferLength(1);
            _buffer[Position++] = value;

            Length = Math.Max(Length, Position);
        }

        [Obsolete("Use WriteUInt8Array.")]
        public void WriteBytes(byte[] value, int offset, int count) => WriteUInt8Array(value, offset, count);

        /// <summary>
        /// Writes bytes.
        /// </summary>
        /// <param name = "value"></param>
        /// <param name = "offset"></param>
        /// <param name = "count"></param>
        public void WriteUInt8Array(byte[] value, int offset, int count)
        {
            EnsureBufferLength(count);
            Buffer.BlockCopy(value, offset, _buffer, Position, count);
            Position += count;
            Length = Math.Max(Length, Position);
        }

        [Obsolete("Use WriteUInt8ArrayAndSize.")]
        public void WriteBytesAndSize(byte[] value, int offset, int count) => WriteUInt8ArrayAndSize(value, offset, count);

        /// <summary>
        /// Writes bytes and length of bytes.
        /// </summary>
        /// <param name = "value"></param>
        /// <param name = "offset"></param>
        /// <param name = "count"></param>
        public void WriteUInt8ArrayAndSize(byte[] value, int offset, int count)
        {
            if (value == null)
            {
                WriteInt32(UNSET_COLLECTION_SIZE_VALUE);
            }
            else
            {
                WriteInt32(count);
                WriteUInt8Array(value, offset, count);
            }
        }

        [Obsolete("Use WriteUInt8ArrayAndSize.")]
        public void WriteBytesAndSize(byte[] value) => WriteUInt8ArrayAndSize(value);

        /// <summary>
        /// Writes all bytes in value and length of bytes.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteUInt8ArrayAndSize(byte[] value)
        {
            int size = value == null ? 0 : value.Length;
            // buffer might be null, so we can't use .Length in that case
            WriteUInt8ArrayAndSize(value, 0, size);
        }

        [Obsolete("Use WriteInt8Unpacked.")]
        public void WriteSByte(sbyte value) => WriteInt8Unpacked(value);

        /// <summary>
        /// Writes a sbyte.
        /// </summary>
        [DefaultWriter]
        public void WriteInt8Unpacked(sbyte value) => WriteUInt8Unpacked((byte)value);

        /// <summary>
        /// Writes a char.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteChar(char value)
        {
            EnsureBufferLength(2);
            _buffer[Position++] = (byte)value;
            _buffer[Position++] = (byte)(value >> 8);
            Length = Math.Max(Length, Position);
        }

        /// <summary>
        /// Writes a boolean.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteBoolean(bool value)
        {
            EnsureBufferLength(1);
            _buffer[Position++] = value ? (byte)1 : (byte)0;
            Length = Math.Max(Length, Position);
        }

        /// <summary>
        /// Writes a uint16 unpacked.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteUInt16Unpacked(ushort value)
        {
            EnsureBufferLength(2);
            _buffer[Position++] = (byte)value;
            _buffer[Position++] = (byte)(value >> 8);
            Length = Math.Max(Length, Position);
        }

        /// <summary>
        /// Writes a uint16.
        /// </summary>
        /// <param name = "value"></param>
        // todo: should be using WritePackedWhole but something relying on unpacked short/ushort is being written packed, corrupting packets.
        [DefaultWriter]
        public void WriteUInt16(ushort value) => WriteUInt16Unpacked(value);

        /// <summary>
        /// Writes a int16 unpacked.
        /// </summary>
        /// <param name = "value"></param>
        // todo: should be WritePackedWhole but something relying on unpacked short/ushort is being written packed, corrupting packets.
        public void WriteInt16Unpacked(short value) => WriteUInt16Unpacked((ushort)value);

        /// <summary>
        /// Writes a int16.
        /// </summary>
        /// <param name = "value"></param>
        // todo: should be WritePackedWhole but something relying on unpacked short/ushort is being written packed, corrupting packets.
        [DefaultWriter]
        public void WriteInt16(short value) => WriteUInt16Unpacked((ushort)value);

        /// <summary>
        /// Writes a int32.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteInt32Unpacked(int value) => WriteUInt32Unpacked((uint)value);

        /// <summary>
        /// Writes an int32.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteInt32(int value) => WriteSignedPackedWhole(value);

        /// <summary>
        /// Writes value to dst without error checking.
        /// </summary>
        internal static void WriteUInt32Unpacked(byte[] dst, uint value, ref int position)
        {
            dst[position++] = (byte)value;
            dst[position++] = (byte)(value >> 8);
            dst[position++] = (byte)(value >> 16);
            dst[position++] = (byte)(value >> 24);
        }

        /// <summary>
        /// Writes a uint32.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteUInt32Unpacked(uint value)
        {
            EnsureBufferLength(4);
            WriteUInt32Unpacked(_buffer, value, ref Position);
            Length = Math.Max(Length, Position);
        }

        /// <summary>
        /// Writes a uint32.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteUInt32(uint value) => WriteUnsignedPackedWhole(value);

        /// <summary>
        /// Writes a uint64.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteUInt64Unpacked(ulong value)
        {
            EnsureBufferLength(8);
            _buffer[Position++] = (byte)value;
            _buffer[Position++] = (byte)(value >> 8);
            _buffer[Position++] = (byte)(value >> 16);
            _buffer[Position++] = (byte)(value >> 24);
            _buffer[Position++] = (byte)(value >> 32);
            _buffer[Position++] = (byte)(value >> 40);
            _buffer[Position++] = (byte)(value >> 48);
            _buffer[Position++] = (byte)(value >> 56);
            Length = Math.Max(Position, Length);
        }

        /// <summary>
        /// Writes a uint64.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteUInt64(ulong value) => WriteUnsignedPackedWhole(value);

        /// <summary>
        /// Writes a int64.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteInt64Unpacked(long value) => WriteUInt64((ulong)value);

        /// <summary>
        /// Writes an int64.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteInt64(long value) => WriteSignedPackedWhole(value);

        /// <summary>
        /// Writes a single (float).
        /// </summary>
        /// <param name = "value"></param>
        public void WriteSingleUnpacked(float value)
        {
            EnsureBufferLength(4);
            UIntFloat converter = new() { FloatValue = value };
            WriteUInt32Unpacked(converter.UIntValue);
        }

        /// <summary>
        /// Writes a single (float).
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteSingle(float value) => WriteSingleUnpacked(value);

        /// <summary>
        /// Writes a double.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteDoubleUnpacked(double value)
        {
            UIntDouble converter = new() { DoubleValue = value };
            WriteUInt64Unpacked(converter.LongValue);
        }

        /// <summary>
        /// Writes a double.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteDouble(double value) => WriteDoubleUnpacked(value);

        /// <summary>
        /// Writes a decimal.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteDecimalUnpacked(decimal value)
        {
            UIntDecimal converter = new() { DecimalValue = value };
            WriteUInt64Unpacked(converter.LongValue1);
            WriteUInt64Unpacked(converter.LongValue2);
        }

        /// <summary>
        /// Writes a decimal.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteDecimal(decimal value) => WriteDecimalUnpacked(value);

        /// <summary>
        /// Writes a string.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteString(string value)
        {
            if (value == null)
            {
                WriteInt32(UNSET_COLLECTION_SIZE_VALUE);
                return;
            }

            byte[] buffer = Strings.Buffer;
            int length = value.ToBytes(ref buffer);

            WriteInt32(length);

            // Nothing to write.
            if (length == 0)
                return;

            WriteUInt8Array(buffer, 0, length);
        }

        /// <summary>
        /// Writes a byte ArraySegment and it's size.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteArraySegmentAndSize(ArraySegment<byte> value) => WriteUInt8ArrayAndSize(value.Array, value.Offset, value.Count);

        /// <summary>
        /// Writes an ArraySegment without size.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteArraySegment(ArraySegment<byte> value) => WriteUInt8Array(value.Array, value.Offset, value.Count);

        /// <summary>
        /// Writes AutoPackType.
        /// </summary>
        [DefaultWriter]
        public void WriteAutoPackType(AutoPackType apt) => WriteUInt8Unpacked((byte)apt);

        /// <summary>
        /// Writes a Vector2.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteVector2Unpacked(Vector2 value)
        {
            WriteSingleUnpacked(value.x);
            WriteSingleUnpacked(value.y);
        }

        /// <summary>
        /// Writes a Vector2.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteVector2(Vector2 value) => WriteVector2Unpacked(value);

        /// <summary>
        /// Writes a Vector3
        /// </summary>
        /// <param name = "value"></param>
        public void WriteVector3Unpacked(Vector3 value)
        {
            WriteSingleUnpacked(value.x);
            WriteSingleUnpacked(value.y);
            WriteSingleUnpacked(value.z);
        }

        /// <summary>
        /// Writes a Vector3
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteVector3(Vector3 value) => WriteVector3Unpacked(value);

        /// <summary>
        /// Writes a Vector4.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteVector4Unpacked(Vector4 value)
        {
            WriteSingleUnpacked(value.x);
            WriteSingleUnpacked(value.y);
            WriteSingleUnpacked(value.z);
            WriteSingleUnpacked(value.w);
        }

        /// <summary>
        /// Writes a Vector4.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteVector4(Vector4 value) => WriteVector4Unpacked(value);

        /// <summary>
        /// Writes a Vector2Int.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteVector2IntUnpacked(Vector2Int value)
        {
            WriteInt32Unpacked(value.x);
            WriteInt32Unpacked(value.y);
        }

        /// <summary>
        /// Writes a Vector2Int.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteVector2Int(Vector2Int value)
        {
            WriteSignedPackedWhole(value.x);
            WriteSignedPackedWhole(value.y);
        }

        /// <summary>
        /// Writes a Vector3Int.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteVector3IntUnpacked(Vector3Int value)
        {
            WriteInt32Unpacked(value.x);
            WriteInt32Unpacked(value.y);
            WriteInt32Unpacked(value.z);
        }

        /// <summary>
        /// Writes a Vector3Int.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteVector3Int(Vector3Int value)
        {
            WriteSignedPackedWhole(value.x);
            WriteSignedPackedWhole(value.y);
            WriteSignedPackedWhole(value.z);
        }

        /// <summary>
        /// Writes a Color.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteColorUnpacked(Color value)
        {
            WriteSingleUnpacked(value.r);
            WriteSingleUnpacked(value.g);
            WriteSingleUnpacked(value.b);
            WriteSingleUnpacked(value.a);
        }

        /// <summary>
        /// Writes a Color.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteColor(Color value)
        {
            EnsureBufferLength(4);
            _buffer[Position++] = (byte)(value.r * 100f);
            _buffer[Position++] = (byte)(value.g * 100f);
            _buffer[Position++] = (byte)(value.b * 100f);
            _buffer[Position++] = (byte)(value.a * 100f);
            Length = Math.Max(Length, Position);
        }

        /// <summary>
        /// Writes a Color32.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteColor32(Color32 value)
        {
            EnsureBufferLength(4);
            _buffer[Position++] = value.r;
            _buffer[Position++] = value.g;
            _buffer[Position++] = value.b;
            _buffer[Position++] = value.a;

            Length = Math.Max(Length, Position);
        }

        /// <summary>
        /// Writes a Quaternion.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteQuaternionUnpacked(Quaternion value)
        {
            WriteSingleUnpacked(value.x);
            WriteSingleUnpacked(value.y);
            WriteSingleUnpacked(value.z);
            WriteSingleUnpacked(value.w);
        }

        /// <summary>
        /// Writes a Quaternion.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteQuaternion64(Quaternion value)
        {
            ulong result = Quaternion64Compression.Compress(value);
            WriteUInt64Unpacked(result);
        }

        /// <summary>
        /// Writes a Quaternion.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteQuaternion32(Quaternion value)
        {
            Quaternion32Compression.Compress(this, value);
        }

        /// <summary>
        /// Reads a Quaternion.
        /// </summary>
        /// <returns></returns>
        public void WriteQuaternion(Quaternion value, AutoPackType autoPackType)
        {
            switch (autoPackType)
            {
                case AutoPackType.Packed:
                    WriteQuaternion32(value);
                    break;
                case AutoPackType.PackedLess:
                    WriteQuaternion64(value);
                    break;
                default:
                    WriteQuaternionUnpacked(value);
                    break;
            }
        }

        /// <summary>
        /// Writes a rect.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteRectUnpacked(Rect value)
        {
            WriteSingleUnpacked(value.xMin);
            WriteSingleUnpacked(value.yMin);
            WriteSingleUnpacked(value.width);
            WriteSingleUnpacked(value.height);
        }

        /// <summary>
        /// Writes a rect.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteRect(Rect value) => WriteRectUnpacked(value);

        /// <summary>
        /// Writes a plane.
        /// </summary>
        /// <param name = "value"></param>
        public void WritePlaneUnpacked(Plane value)
        {
            WriteVector3Unpacked(value.normal);
            WriteSingleUnpacked(value.distance);
        }

        /// <summary>
        /// Writes a plane.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WritePlane(Plane value) => WritePlaneUnpacked(value);

        /// <summary>
        /// Writes a Ray.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteRayUnpacked(Ray value)
        {
            WriteVector3Unpacked(value.origin);
            WriteVector3Unpacked(value.direction);
        }

        /// <summary>
        /// Writes a Ray.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteRay(Ray value) => WriteRayUnpacked(value);

        /// <summary>
        /// Writes a Ray2D.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteRay2DUnpacked(Ray2D value)
        {
            WriteVector2Unpacked(value.origin);
            WriteVector2Unpacked(value.direction);
        }

        /// <summary>
        /// Writes a Ray2D.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteRay2D(Ray2D value) => WriteRay2DUnpacked(value);

        /// <summary>
        /// Writes a Matrix4x4.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteMatrix4x4Unpacked(Matrix4x4 value)
        {
            WriteSingleUnpacked(value.m00);
            WriteSingleUnpacked(value.m01);
            WriteSingleUnpacked(value.m02);
            WriteSingleUnpacked(value.m03);
            WriteSingleUnpacked(value.m10);
            WriteSingleUnpacked(value.m11);
            WriteSingleUnpacked(value.m12);
            WriteSingleUnpacked(value.m13);
            WriteSingleUnpacked(value.m20);
            WriteSingleUnpacked(value.m21);
            WriteSingleUnpacked(value.m22);
            WriteSingleUnpacked(value.m23);
            WriteSingleUnpacked(value.m30);
            WriteSingleUnpacked(value.m31);
            WriteSingleUnpacked(value.m32);
            WriteSingleUnpacked(value.m33);
        }

        /// <summary>
        /// Writes a Matrix4x4.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteMatrix4x4(Matrix4x4 value) => WriteMatrix4x4Unpacked(value);

        /// <summary>
        /// Writes a Guid.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteGuid(Guid value)
        {
            byte[] data = _guidBuffer;
            value.TryWriteBytes(data);
            WriteUInt8Array(data, 0, data.Length);
        }

        [Obsolete("Use WriteGuid instead.")]
        public void WriteGuidAllocated(Guid value) => WriteGuid(value);

        /// <summary>
        /// Writes a tick without packing.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteTickUnpacked(uint value) => WriteUInt32Unpacked(value);

        /// <summary>
        /// Writes a GameObject. GameObject must be spawned over the network already or be a prefab with a NetworkObject attached.
        /// </summary>
        /// <param name = "go"></param>
        [DefaultWriter]
        public void WriteGameObject(GameObject go)
        {
            // There needs to be a header to indicate if null, nob, or nb.
            if (go == null)
            {
                WriteUInt8Unpacked(0);
            }
            else
            {
                // Try to write the NetworkObject first.
                if (go.TryGetComponent(out NetworkObject nob))
                {
                    WriteUInt8Unpacked(1);
                    WriteNetworkObject(nob);
                }
                // If there was no nob try to write a NetworkBehaviour.
                else if (go.TryGetComponent(out NetworkBehaviour nb))
                {
                    WriteUInt8Unpacked(2);
                    WriteNetworkBehaviour(nb);
                }
                // Object cannot be serialized so write null.
                else
                {
                    WriteUInt8Unpacked(0);
                    NetworkManager.LogError($"GameObject {go.name} cannot be serialized because it does not have a NetworkObject nor NetworkBehaviour.");
                }
            }
        }

        /// <summary>
        /// Writes a Transform. Transform must be spawned over the network already or be a prefab with a NetworkObject attached.
        /// </summary>
        /// <param name = "t"></param>
        [DefaultWriter]
        public void WriteTransform(Transform t)
        {
            if (t == null)
            {
                WriteNetworkObject(null);
            }
            else
            {
                NetworkObject nob = t.GetComponent<NetworkObject>();
                WriteNetworkObject(nob);
            }
        }

        /// <summary>
        /// Writes a NetworkObject.ObjectId.
        /// </summary>
        /// <param name = "nob"></param>
        public void WriteNetworkObjectId(NetworkObject nob)
        {
            int id = nob == null ? NetworkObject.UNSET_OBJECTID_VALUE : nob.ObjectId;
            WriteNetworkObjectId(id);
        }

        /// <summary>
        /// Writes a NetworkObject while optionally including the initialization order.
        /// </summary>
        [DefaultWriter]
        public void WriteNetworkObject(NetworkObject nob)
        {
            if (nob == null)
            {
                WriteNullReferenceId();
                return;
            }

            bool spawned = nob.IsSpawned;

            if (!spawned)
            {
                /* If not spawned and IsInitializedNested is true
                 * we must send as a null reference rather than
                 * the prefabId. Even though the nob might
                 * be a prefab, it's being written as a child
                 * in this case. */
                if (nob.IsInitializedNested)
                {
                    WriteNullReferenceId();
                    return;
                }

                WriteNetworkObjectId(nob.PrefabId);
            }
            else
            {
                WriteNetworkObjectId(nob.ObjectId);
            }

            /* Spawned is written after because it's only needed if nob
             * is not null. If it were written before it would also have
             * to be written when nob == null.*/
            WriteBoolean(spawned);

            void WriteNullReferenceId() => WriteNetworkObjectId(NetworkObject.UNSET_OBJECTID_VALUE);
        }

        /// <summary>
        /// Writes a spawned networkObject.
        /// </summary>
        internal void WriteSpawnedNetworkObject(NetworkObject nob)
        {
            WriteNetworkObjectId(nob.ObjectId);
            WriteUInt16(nob.SpawnableCollectionId);
            WriteInt32(nob.GetInitializeOrder());
        }

        /// <summary>
        /// Writes a NetworkObject for a despawn message.
        /// </summary>
        /// <param name = "nob"></param>
        /// <param name = "dt"></param>
        internal void WriteNetworkObjectForDespawn(NetworkObject nob, DespawnType dt)
        {
            WriteNetworkObjectId(nob.ObjectId);
            WriteUInt8Unpacked((byte)dt);
        }

        /// <summary>
        /// Writes an objectId.
        /// </summary>
        public void WriteNetworkObjectId(int objectId) => WriteSignedPackedWhole(objectId);

        /// <summary>
        /// Writes a NetworkBehaviour.
        /// </summary>
        /// <param name = "nb"></param>
        [DefaultWriter]
        public void WriteNetworkBehaviour(NetworkBehaviour nb)
        {
            if (nb == null)
            {
                WriteNetworkObject(null);
                WriteUInt8Unpacked(0);
            }
            else
            {
                WriteNetworkObject(nb.NetworkObject);
                WriteUInt8Unpacked(nb.ComponentIndex);
            }
        }

        /// <summary>
        /// Writes a NetworkBehaviourId.
        /// </summary>
        public void WriteNetworkBehaviourId(NetworkBehaviour nb)
        {
            if (nb == null)
                WriteUInt8Unpacked(NetworkBehaviour.UNSET_NETWORKBEHAVIOUR_ID);
            else
                WriteUInt8Unpacked(nb.ComponentIndex);
        }

        /// <summary>
        /// Writes a DateTime.
        /// </summary>
        [DefaultWriter]
        public void WriteDateTime(DateTime dt) => WriteSignedPackedWhole(dt.ToBinary());

        /// <summary>
        /// Writes a transport channel.
        /// </summary>
        /// <param name = "channel"></param>
        [DefaultWriter]
        public void WriteChannel(Channel channel) => WriteUInt8Unpacked((byte)channel);

        /// <summary>
        /// Writers a LayerMask.
        /// </summary>
        /// <param name = "value"></param>
        [DefaultWriter]
        public void WriteLayerMask(LayerMask value) => WriteSignedPackedWhole(value.value);

        /// <summary>
        /// Writes a NetworkConnection.
        /// </summary>
        /// <param name = "connection"></param>
        [DefaultWriter]
        public void WriteNetworkConnection(NetworkConnection connection)
        {
            int value = connection == null ? NetworkConnection.UNSET_CLIENTID_VALUE : connection.ClientId;
            WriteNetworkConnectionId(value);
        }

        /// <summary>
        /// Writes TransformProperties.
        /// </summary>
        [DefaultWriter]
        public void WriteTransformProperties(TransformProperties value)
        {
            WriteVector3(value.Position);
            WriteQuaternion32(value.Rotation);
            WriteVector3(value.Scale);
        }

        /// <summary>
        /// Writes a short for a connectionId.
        /// </summary>
        /// <returns></returns>
        public void WriteNetworkConnectionId(int id) => WriteSignedPackedWhole(id);

        /// <summary>
        /// Writes a dictionary.
        /// </summary>
        public void WriteDictionary<TKey, TValue>(Dictionary<TKey, TValue> dict)
        {
            if (dict == null)
            {
                WriteSignedPackedWhole(UNSET_COLLECTION_SIZE_VALUE);
                return;
            }
            else
            {
                WriteSignedPackedWhole(dict.Count);
            }

            foreach (KeyValuePair<TKey, TValue> item in dict)
            {
                Write(item.Key);
                Write(item.Value);
            }
        }

        /// <summary>
        /// Writes a state update packet.
        /// </summary>
        /// <param name = "tick"></param>
        internal void WriteStateUpdatePacket(uint lastPacketTick) => WriteTickUnpacked(lastPacketTick);

        #region Packed writers.
        /// <summary>
        /// ZigZag encode an integer. Move the sign bit to the right.
        /// </summary>
        public ulong ZigZagEncode(ulong value)
        {
            if (value >> 63 > 0)
                return ~(value << 1) | 1;
            return value << 1;
        }

        /// <summary>
        /// Writes a packed whole number.
        /// </summary>
        /// <param name = "value"></param>
        public void WriteSignedPackedWhole(long value) => WriteUnsignedPackedWhole(ZigZagEncode((ulong)value));

        /// <summary>
        /// Writes a packed whole number.
        /// </summary>
        /// <param name = "value"></param>
        /// <summary>
        /// Writes a packed whole number.
        /// </summary>
        /// <param name = "value"> </param>
        public void WriteUnsignedPackedWhole(ulong value)
        {
            EnsureBufferLength(10);
            while (value > 127)
            {
                _buffer[Position++] = (byte)((value & 0x7F) | 0x80);
                value >>= 7;
            }

            _buffer[Position++] = (byte)(value & 0x7F);
            Length = Math.Max(Length, Position);
        }
        #endregion

        #region Generators.
        /// <summary>
        /// Writes a list.
        /// </summary>
        /// <param name = "value">Collection to write.</param>
        /// <param name = "offset">Offset to begin at.</param>
        /// <param name = "count">Entries to write.</param>
        public void WriteList<T>(List<T> value, int offset, int count)
        {
            if (value == null)
            {
                WriteSignedPackedWhole(UNSET_COLLECTION_SIZE_VALUE);
            }
            else
            {
                // Make sure values cannot cause out of bounds.
                if (offset + count > value.Count)
                    count = 0;

                WriteSignedPackedWhole(count);
                for (int i = 0; i < count; i++)
                    Write(value[i + offset]);
            }
        }

        /// <summary>
        /// Writes a list.
        /// </summary>
        /// <param name = "value">Collection to write.</param>
        /// <param name = "offset">Offset to begin at.</param>
        public void WriteList<T>(List<T> value, int offset)
        {
            int count = value == null ? 0 : value.Count;
            WriteList(value, offset, count - offset);
        }

        /// <summary>
        /// Writes a list.
        /// </summary>
        /// <param name = "value">Collection to write.</param>
        public void WriteList<T>(List<T> value)
        {
            int count = value == null ? 0 : value.Count;
            WriteList(value, 0, count);
        }

        /// <summary>
        /// Writes a collection.
        /// </summary>
        /// <param name = "value">Collection to write.</param>
        public void WriteHashSet<T>(HashSet<T> value)
        {
            /* HashSet cannot be index iterated so
             * there is no need for an offset/count
             * overload. */

            if (value == null)
            {
                WriteSignedPackedWhole(UNSET_COLLECTION_SIZE_VALUE);
            }
            else
            {
                WriteSignedPackedWhole(value.Count);

                foreach (T item in value)
                    Write(item);
            }
        }

        /// <summary>
        /// Writes an array.
        /// </summary>
        /// <param name = "value">Collection to write.</param>
        public void WriteArray<T>(T[] value)
        {
            int count = value == null ? 0 : value.Length;
            WriteArray(value, 0, count);
        }

        /// <summary>
        /// Writes an array.
        /// </summary>
        /// <param name = "value">Collection to write.</param>
        /// <param name = "offset">Offset to begin at.</param>
        public void WriteArray<T>(T[] value, int offset)
        {
            int count = value == null ? 0 : value.Length;
            WriteArray(value, offset, count - offset);
        }

        /// <summary>
        /// Writes an array.
        /// </summary>
        /// <param name = "value">Collection to write.</param>
        /// <param name = "offset">Offset to begin at.</param>
        /// <param name = "count">Entries to write.</param>
        public void WriteArray<T>(T[] value, int offset, int count)
        {
            if (value == null)
            {
                WriteSignedPackedWhole(UNSET_COLLECTION_SIZE_VALUE);
            }
            else
            {
                // If theres no values, or offset exceeds count then write 0 for count.
                if (value.Length == 0 || offset >= count)
                {
                    WriteSignedPackedWhole(0);
                }
                else
                {
                    WriteSignedPackedWhole(count);
                    for (int i = offset; i < count; i++)
                        Write(value[i]);
                }
            }
        }

        /// <summary>
        /// Writes a reconcile.
        /// </summary>
        internal void WriteReconcile<T>(T data)
        {
            Write(data);
        }

        /// <summary>
        /// Writes a replication to the server.
        /// </summary>
        internal void WriteReplicate<T>(RingBuffer<ReplicateDataContainer<T>> values, int offset) where T : IReplicateData, new()
        {
            /* COUNT
             *
             * Each Entry:
             * 0 if the same as previous.
             * 1 if default. */
            int collectionCount = values.Count;
            // Replicate list will never be null, no need to write null check.
            // Number of entries being written.
            byte count = (byte)(collectionCount - offset);
            WriteUInt8Unpacked(count);

            for (int i = offset; i < collectionCount; i++)
                WriteReplicateDataContainer<T>(values[i]);
        }

        internal void WriteReplicate<T>(BasicQueue<ReplicateDataContainer<T>> values, int redundancyCount) where T : IReplicateData, new()
        {
            /* COUNT
             *
             * Each Entry:
             * 0 if the same as previous.
             * 1 if default. */
            int collectionCount = values.Count;
            // Replicate list will never be null, no need to write null check.
            // Number of entries being written.
            byte count = (byte)redundancyCount;
            WriteUInt8Unpacked(count);

            for (int i = collectionCount - redundancyCount; i < collectionCount; i++)
                WriteReplicateDataContainer<T>(values[i]);
        }

        /// <summary>
        /// Reads a ReplicateData and applies tick and channel.
        /// </summary>
        private void WriteReplicateDataContainer<T>(ReplicateDataContainer<T> value) where T : IReplicateData, new()
        {
            Write<T>(value.Data);
            WriteChannel(value.Channel);
        }

        /// <summary>
        /// Writes any supported type using packing.
        /// </summary>
        public void Write<T>(T value)
        {
            Action<Writer, T> del = GenericWriter<T>.Write;
            if (del == null)
                NetworkManager.LogError($"Write method not found for {typeof(T).FullName}. Use a supported type or create a custom serializer.");
            else
                del.Invoke(this, value);
        }
        #endregion
    }
}