#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
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
using UnityEngine;

[assembly: InternalsVisibleTo(UtilityConstants.GENERATED_ASSEMBLY_NAME)]
// Required for internal tests.
[assembly: InternalsVisibleTo(UtilityConstants.TEST_ASSEMBLY_NAME)]

namespace FishNet.Serializing
{
    /// <summary>
    /// Reads data from a buffer.
    /// </summary>
    public partial class Reader
    {
        #region Types.
        public enum DataSource
        {
            Unset = 0,
            Server = 1,
            Client = 2
        }
        #endregion

        #region Public.
        /// <summary>
        /// Which part of the network the data came from.
        /// </summary>
        public DataSource Source = DataSource.Unset;
        /// <summary>
        /// Capacity of the buffer.
        /// </summary>
        public int Capacity => _buffer.Length;
        /// <summary>
        /// NetworkManager for this reader. Used to lookup objects.
        /// </summary>
        public NetworkManager NetworkManager;
        /// <summary>
        /// Offset within the buffer when the reader was created.
        /// </summary>
        public int Offset { get; private set; }
        /// <summary>
        /// Position for the next read.
        /// </summary>
        public int Position;
        /// <summary>
        /// Total number of bytes available within the buffer.
        /// </summary>
        public int Length { get; private set; }
        /// <summary>
        /// Bytes remaining to be read. This value is Length - Position.
        /// </summary>
        public int Remaining => Length + Offset - Position;
        #endregion

        #region Internal.
        /// <summary>
        /// NetworkConnection that this data came from.
        /// Value may not always be set.
        /// </summary>
        public NetworkConnection NetworkConnection { get; private set; }
        #if DEVELOPMENT
        /// <summary>
        /// Last NetworkObject parsed.
        /// </summary>
        public static NetworkObject LastNetworkObject { get; private set; }
        /// <summary>
        /// Last NetworkBehaviour parsed.
        /// </summary>
        public static NetworkBehaviour LastNetworkBehaviour { get; private set; }
        #endif
        #endregion

        #region Private.
        /// <summary>
        /// Data being read.
        /// </summary>
        private byte[] _buffer;
        /// <summary>
        /// Used to convert bytes to a string.
        /// </summary>
        private static readonly UTF8Encoding _encoding = new(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true);
        /// <summary>
        /// Used to convert bytes to a GUID.
        /// </summary>
        private static readonly byte[] _guidBuffer = new byte[16];
        // /// <summary>
        // /// True if the data being read is from a remote client; a remote client means not clientHost, and not server.
        // /// </summary>
        // private bool _isFromRemoteClient;
        #endregion

        public Reader() { }

        public Reader(byte[] bytes, NetworkManager networkManager, NetworkConnection networkConnection = null, DataSource source = DataSource.Unset)
        {
            Initialize(bytes, networkManager, networkConnection, source);
        }
        
        public Reader(ArraySegment<byte> segment, NetworkManager networkManager, NetworkConnection networkConnection = null, DataSource source = DataSource.Unset)
        {
            Initialize(segment, networkManager, networkConnection, source);
        }
        
        /// Outputs reader to string.
        /// </summary>
        /// <returns></returns>
        public override string ToString() => ToString(0, Length);

        /// <summary>
        /// Outputs reader to string starting at an index.
        /// </summary>
        /// <returns></returns>
        public string ToString(int offset, int length)
        {
            return $"Position: {Position:0000}, Length: {Length:0000}, Buffer: {BitConverter.ToString(_buffer, offset, length)}.";
        }

        /// <summary>
        /// Outputs reader to string.
        /// </summary>
        /// <returns></returns>
        public string RemainingToString()
        {
            string buffer = Remaining > 0 ? BitConverter.ToString(_buffer, Position, Remaining) : "null";
            return $"Remaining: {Remaining}, Length: {Length}, Buffer: {buffer}.";
        }

        /// <summary>
        /// Returns remaining data as an ArraySegment.
        /// </summary>
        /// <returns></returns>
        public ArraySegment<byte> GetRemainingData()
        {
            if (Remaining == 0)
                return default;

            return new(_buffer, Position, Remaining);
        }

        /// <summary>
        /// Initializes this reader with data.
        /// </summary>
        public void Initialize(ArraySegment<byte> segment, NetworkManager networkManager, DataSource source = DataSource.Unset)
        {
            Initialize(segment, networkManager, sender: null, source);
        }

        
        /// <summary>
        /// Initializes this reader with data.
        /// </summary>
        public void Initialize(ArraySegment<byte> segment, NetworkManager networkManager, NetworkConnection sender = null, DataSource source = DataSource.Unset)
        {
            _buffer = segment.Array;
            if (_buffer == null)
                _buffer = Array.Empty<byte>();

            Position = segment.Offset;
            Offset = segment.Offset;
            Length = segment.Count;

            NetworkManager = networkManager;
            NetworkConnection = sender;
            Source = source;
        }
    
        /// <summary>
        /// Initializes this reader with data.
        /// </summary>
        public void Initialize(byte[] bytes, NetworkManager networkManager, DataSource source = DataSource.Unset)
        {
            Initialize(new ArraySegment<byte>(bytes), networkManager, null, source);
        }
        
        /// <summary>
        /// Initializes this reader with data.
        /// </summary>
        public void Initialize(byte[] bytes, NetworkManager networkManager, NetworkConnection sender = null, DataSource source = DataSource.Unset)
        {
            Initialize(new ArraySegment<byte>(bytes), networkManager, sender, source);
        }
        
        
        /// <summary>
        /// Reads length. This method is used to make debugging easier.
        /// </summary>
        internal int ReadLength()
        {
            return ReadInt32();
        }

        /// <summary>
        /// Reads a packetId.
        /// </summary>
        internal PacketId ReadPacketId()
        {
            return (PacketId)ReadUInt16Unpacked();
        }

        /// <summary>
        /// Returns a ushort without advancing the reader.
        /// </summary>
        /// <returns></returns>
        internal PacketId PeekPacketId()
        {
            int currentPosition = Position;
            PacketId result = ReadPacketId();
            Position = currentPosition;
            return result;
        }

        /// <summary>
        /// Returns the next byte to be read.
        /// </summary>
        /// <returns></returns>
        internal byte PeekUInt8()
        {
            return _buffer[Position];
        }

        /// <summary>
        /// Skips a number of bytes in the reader.
        /// </summary>
        /// <param name = "value">Number of bytes to skip.</param>
        public void Skip(int value)
        {
            if (value < 1 || Remaining < value)
                return;

            Position += value;
        }

        /// <summary>
        /// Clears remaining bytes to be read.
        /// </summary>
        public void Clear()
        {
            if (Remaining > 0)
                Skip(Remaining);
        }

        /// <summary>
        /// Returns the buffer as an ArraySegment.
        /// </summary>
        /// <returns></returns>
        public ArraySegment<byte> GetArraySegmentBuffer()
        {
            return new(_buffer, Offset, Length);
        }

        [Obsolete("Use GetBuffer.")] // Remove V5
        public byte[] GetByteBuffer() => GetBuffer();

        /// <summary>
        /// Returns the buffer as bytes. This does not trim excessive bytes.
        /// </summary>
        /// <returns></returns>
        public byte[] GetBuffer()
        {
            return _buffer;
        }

        [Obsolete("Use GetBufferAllocated().")] // Remove V5
        public byte[] GetByteBufferAllocated() => GetBufferAllocated();

        /// <summary>
        /// Returns the buffer as bytes and allocates into a new array.
        /// </summary>
        /// <returns></returns>
        [Obsolete("Use GetBufferAllocated().")] // Remove V5
        public byte[] GetBufferAllocated()
        {
            byte[] result = new byte[Length];
            Buffer.BlockCopy(_buffer, Offset, result, 0, Length);
            return result;
        }

        /// <summary>
        /// BlockCopies data from the reader to target and advances reader.
        /// </summary>
        /// <param name = "target"></param>
        /// <param name = "targetOffset"></param>
        /// <param name = "count"></param>
        public void BlockCopy(ref byte[] target, int targetOffset, int count)
        {
            Buffer.BlockCopy(_buffer, Position, target, targetOffset, count);
            Position += count;
        }

        [Obsolete("Use ReadUInt8Unpacked.")] // Remove in V5.
        public byte ReadByte() => ReadUInt8Unpacked();

        /// <summary>
        /// Reads a byte.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public byte ReadUInt8Unpacked()
        {
            byte r = _buffer[Position];
            Position += 1;
            return r;
        }

        [Obsolete("Use ReadUInt8ArrayAllocated.")]
        public byte[] ReadBytesAllocated(int count) => ReadUInt8ArrayAllocated(count);

        [Obsolete("Use ReadUInt8Array.")]
        public void ReadBytes(ref byte[] buffer, int count) => ReadUInt8Array(ref buffer, count);

        /// <summary>
        /// Read bytes from position into target.
        /// </summary>
        /// <param name = "buffer">Buffer to read bytes into.</param>
        /// <param name = "count">Number of bytes to read.</param>
        public void ReadUInt8Array(ref byte[] buffer, int count)
        {
            if (buffer == null)
                NetworkManager.LogError($"Buffer cannot be null.");
            else if (count > buffer.Length)
                NetworkManager.LogError($"Count of {count} exceeds target length of {buffer.Length}.");
            else
                BlockCopy(ref buffer, 0, count);
        }

        /// <summary>
        /// Creates an ArraySegment by reading a number of bytes from position.
        /// </summary>
        /// <param name = "count"></param>
        /// <returns></returns>
        public ArraySegment<byte> ReadArraySegment(int count)
        {
            if (count < 0)
            {
                NetworkManager.Log($"ArraySegment count cannot be less than 0.");
                // Purge renaming and return default.
                Position += Remaining;
                return default;
            }

            if (IsPossibleAllocationAttack(count, movePositionOnTrue: true))
                return default;

            ArraySegment<byte> result = new(_buffer, Position, count);
            Position += count;
            return result;
        }

        [Obsolete("Use ReadInt8Unpacked.")] // Remove in V5.
        public sbyte ReadSByte() => ReadInt8Unpacked();

        /// <summary>
        /// Reads a sbyte.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public sbyte ReadInt8Unpacked() => (sbyte)ReadUInt8Unpacked();

        /// <summary>
        /// Reads a char.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public char ReadChar() => (char)ReadUInt16();

        /// <summary>
        /// Reads a boolean.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public bool ReadBoolean()
        {
            byte result = ReadUInt8Unpacked();
            return result == 1 ? true : false;
        }

        /// <summary>
        /// Reads an int16.
        /// </summary>
        /// <returns></returns>
        public ushort ReadUInt16Unpacked()
        {
            ushort result = 0;
            result |= _buffer[Position++];
            result |= (ushort)(_buffer[Position++] << 8);

            return result;
        }

        /// <summary>
        /// Reads an int16.
        /// </summary>
        /// <returns></returns>
        // todo: should be using ReadPackedWhole but something relying on unpacked short/ushort is being written packed, corrupting packets.
        [DefaultReader]
        public ushort ReadUInt16() => ReadUInt16Unpacked();

        /// <summary>
        /// Reads a uint16.
        /// </summary>
        /// <returns></returns>
        // todo: should be using ReadPackedWhole but something relying on unpacked short/ushort is being written packed, corrupting packets.
        public short ReadInt16Unpacked() => (short)ReadUInt16Unpacked();

        /// <summary>
        /// Reads a uint16.
        /// </summary>
        /// <returns></returns>
        // todo: should be using ReadPackedWhole but something relying on unpacked short/ushort is being written packed, corrupting packets.
        [DefaultReader]
        public short ReadInt16() => (short)ReadUInt16Unpacked();

        /// <summary>
        /// Reads an int32.
        /// </summary>
        /// <returns></returns>
        public uint ReadUInt32Unpacked()
        {
            uint result = 0;
            result |= _buffer[Position++];
            result |= (uint)_buffer[Position++] << 8;
            result |= (uint)_buffer[Position++] << 16;
            result |= (uint)_buffer[Position++] << 24;

            return result;
        }

        /// <summary>
        /// Reads an int32.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public uint ReadUInt32() => (uint)ReadUnsignedPackedWhole();

        /// <summary>
        /// Reads a uint32.
        /// </summary>
        /// <returns></returns>
        public int ReadInt32Unpacked() => (int)ReadUInt32Unpacked();

        /// <summary>
        /// Reads a uint32.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public int ReadInt32() => (int)ReadSignedPackedWhole();

        /// <summary>
        /// Reads a uint64.
        /// </summary>
        /// <returns></returns>
        public long ReadInt64Unpacked() => (long)ReadUInt64Unpacked();

        /// <summary>
        /// Reads a uint64.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public long ReadInt64() => (long)ReadSignedPackedWhole();

        /// <summary>
        /// Reads an int64.
        /// </summary>
        /// <returns></returns>
        public ulong ReadUInt64Unpacked()
        {
            ulong result = 0;
            result |= _buffer[Position++];
            result |= (ulong)_buffer[Position++] << 8;
            result |= (ulong)_buffer[Position++] << 16;
            result |= (ulong)_buffer[Position++] << 24;
            result |= (ulong)_buffer[Position++] << 32;
            result |= (ulong)_buffer[Position++] << 40;
            result |= (ulong)_buffer[Position++] << 48;
            result |= (ulong)_buffer[Position++] << 56;

            return result;
        }

        /// <summary>
        /// Reads an int64.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public ulong ReadUInt64() => ReadUnsignedPackedWhole();

        /// <summary>
        /// Reads a single.
        /// </summary>
        /// <returns></returns>
        public float ReadSingleUnpacked()
        {
            UIntFloat converter = new();
            converter.UIntValue = ReadUInt32Unpacked();
            return converter.FloatValue;
        }

        /// <summary>
        /// Reads a single.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public float ReadSingle() => ReadSingleUnpacked();

        /// <summary>
        /// Reads a double.
        /// </summary>
        /// <returns></returns>
        public double ReadDoubleUnpacked()
        {
            UIntDouble converter = new();
            converter.LongValue = ReadUInt64Unpacked();
            return converter.DoubleValue;
        }

        /// <summary>
        /// Reads a double.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public double ReadDouble() => ReadDoubleUnpacked();

        /// <summary>
        /// Reads a decimal.
        /// </summary>
        /// <returns></returns>
        public decimal ReadDecimalUnpacked()
        {
            UIntDecimal converter = new();
            converter.LongValue1 = ReadUInt64Unpacked();
            converter.LongValue2 = ReadUInt64Unpacked();
            return converter.DecimalValue;
        }

        /// <summary>
        /// Reads a decimal.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public decimal ReadDecimal() => ReadDecimalUnpacked();

        [Obsolete("use ReadStringAllocated.")]
        public string ReadString() => ReadStringAllocated();

        /// <summary>
        /// Reads a string.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public string ReadStringAllocated()
        {
            int length = ReadInt32();
            // Null string.
            if (length == Writer.UNSET_COLLECTION_SIZE_VALUE)
                return null;

            if (length == 0)
                return string.Empty;
            if (IsPossibleAllocationAttack(length, movePositionOnTrue: true))
                return string.Empty;

            ArraySegment<byte> data = ReadArraySegment(length);
            return data.Array.ToString(data.Offset, data.Count);
        }

        [Obsolete("Use ReadUInt8ArrayAndSizeAllocated.")]
        public byte[] ReadBytesAndSizeAllocated() => ReadUInt8ArrayAndSizeAllocated();

        /// <summary>
        /// Creates a byte array and reads bytes and size into it.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public byte[] ReadUInt8ArrayAndSizeAllocated()
        {
            int size = ReadInt32();
            if (size == Writer.UNSET_COLLECTION_SIZE_VALUE)
                return null;

            return ReadUInt8ArrayAllocated(size);
        }

        [Obsolete("Use ReadUInt8ArrayAndSize.")]
        public int ReadBytesAndSize(ref byte[] target) => ReadUInt8ArrayAndSize(ref target);

        /// <summary>
        /// Reads bytes and size and copies results into target. Returns UNSET if null was written.
        /// </summary>
        /// <returns>Bytes read.</returns>
        public int ReadUInt8ArrayAndSize(ref byte[] target)
        {
            int size = ReadInt32();
            if (size > 0)
                ReadUInt8Array(ref target, size);

            return size;
        }

        /// <summary>
        /// Reads bytes and size and returns as an ArraySegment.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public ArraySegment<byte> ReadArraySegmentAndSize()
        {
            int size = ReadInt32();
            /* UNSET would be written for null. But since
             * ArraySegments cannot be null return default if
             * length is unset or 0. */
            if (size == Writer.UNSET_COLLECTION_SIZE_VALUE)
                return default;

            return ReadArraySegment(size);
        }

        /// <summary>
        /// Reads AutoPackType.
        /// </summary>
        [DefaultReader]
        public AutoPackType ReadAutoPackType() => (AutoPackType)ReadUInt8Unpacked();

        /// <summary>
        /// Reads a Vector2.
        /// </summary>
        /// <returns></returns>
        public Vector2 ReadVector2Unpacked() => new(ReadSingleUnpacked(), ReadSingleUnpacked());

        /// <summary>
        /// Reads a Vector2.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Vector2 ReadVector2() => ReadVector2Unpacked();

        /// <summary>
        /// Reads a Vector3.
        /// </summary>
        /// <returns></returns>
        public Vector3 ReadVector3Unpacked() => new(ReadSingleUnpacked(), ReadSingleUnpacked(), ReadSingleUnpacked());

        /// <summary>
        /// Reads a Vector3.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Vector3 ReadVector3() => ReadVector3Unpacked();

        /// <summary>
        /// Reads a Vector4.
        /// </summary>
        /// <returns></returns>
        public Vector4 ReadVector4Unpacked() => new(ReadSingleUnpacked(), ReadSingleUnpacked(), ReadSingleUnpacked(), ReadSingleUnpacked());

        /// <summary>
        /// Reads a Vector4.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Vector4 ReadVector4() => ReadVector4Unpacked();

        /// <summary>
        /// Reads a Vector2Int.
        /// </summary>
        /// <returns></returns>
        public Vector2Int ReadVector2IntUnpacked() => new(ReadInt32Unpacked(), ReadInt32Unpacked());

        /// <summary>
        /// Reads a Vector2Int.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Vector2Int ReadVector2Int() => new((int)ReadSignedPackedWhole(), (int)ReadSignedPackedWhole());

        /// <summary>
        /// Reads a Vector3Int.
        /// </summary>
        /// <returns></returns>
        public Vector3Int ReadVector3IntUnpacked() => new(ReadInt32Unpacked(), ReadInt32Unpacked(), ReadInt32Unpacked());

        /// <summary>
        /// Reads a Vector3Int.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Vector3Int ReadVector3Int() => new((int)ReadSignedPackedWhole(), (int)ReadSignedPackedWhole(), (int)ReadSignedPackedWhole());

        /// <summary>
        /// Reads a color.
        /// </summary>
        /// <returns></returns>
        public Color ReadColorUnpacked()
        {
            float r = ReadSingleUnpacked();
            float g = ReadSingleUnpacked();
            float b = ReadSingleUnpacked();
            float a = ReadSingleUnpacked();

            return new(r, g, b, a);
        }

        /// <summary>
        /// Reads a color.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Color ReadColor()
        {
            float r = (float)(ReadUInt8Unpacked() / 100f);
            float g = (float)(ReadUInt8Unpacked() / 100f);
            float b = (float)(ReadUInt8Unpacked() / 100f);
            float a = (float)(ReadUInt8Unpacked() / 100f);

            return new(r, g, b, a);
        }

        /// <summary>
        /// Reads a Color32.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Color32 ReadColor32() => new(ReadUInt8Unpacked(), ReadUInt8Unpacked(), ReadUInt8Unpacked(), ReadUInt8Unpacked());

        /// <summary>
        /// Reads a Quaternion.
        /// </summary>
        /// <returns></returns>
        public Quaternion ReadQuaternionUnpacked() => new(ReadSingleUnpacked(), ReadSingleUnpacked(), ReadSingleUnpacked(), ReadSingleUnpacked());

        /// <summary>
        /// Reads a Quaternion.
        /// </summary>
        /// <returns></returns>
        public Quaternion ReadQuaternion64()
        {
            ulong result = ReadUInt64Unpacked();
            return Quaternion64Compression.Decompress(result);
        }

        /// <summary>
        /// Reads a Quaternion.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Quaternion ReadQuaternion32()
        {
            return Quaternion32Compression.Decompress(this);
        }

        /// <summary>
        /// Reads a Quaternion.
        /// </summary>
        /// <returns></returns>
        public Quaternion ReadQuaternion(AutoPackType autoPackType)
        {
            switch (autoPackType)
            {
                case AutoPackType.Packed:
                    return ReadQuaternion32();
                case AutoPackType.PackedLess:
                    return ReadQuaternion64();
                default:
                    return ReadQuaternionUnpacked();
            }
        }

        /// <summary>
        /// Reads a Rect.
        /// </summary>
        /// <returns></returns>
        public Rect ReadRectUnpacked() => new(ReadSingleUnpacked(), ReadSingleUnpacked(), ReadSingleUnpacked(), ReadSingleUnpacked());

        /// <summary>
        /// Reads a Rect.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Rect ReadRect() => ReadRectUnpacked();

        /// <summary>
        /// Plane.
        /// </summary>
        /// <returns></returns>
        public Plane ReadPlaneUnpacked() => new(ReadVector3Unpacked(), ReadSingleUnpacked());

        /// <summary>
        /// Plane.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Plane ReadPlane() => ReadPlaneUnpacked();

        /// <summary>
        /// Reads a Ray.
        /// </summary>
        /// <returns></returns>
        public Ray ReadRayUnpacked()
        {
            Vector3 position = ReadVector3Unpacked();
            Vector3 direction = ReadVector3Unpacked();
            return new(position, direction);
        }

        /// <summary>
        /// Reads a Ray.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Ray ReadRay() => ReadRayUnpacked();

        /// <summary>
        /// Reads a Ray.
        /// </summary>
        /// <returns></returns>
        public Ray2D ReadRay2DUnpacked()
        {
            Vector3 position = ReadVector2Unpacked();
            Vector2 direction = ReadVector2Unpacked();
            return new(position, direction);
        }

        /// <summary>
        /// Reads a Ray.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Ray2D ReadRay2D() => ReadRay2DUnpacked();

        /// <summary>
        /// Reads a Matrix4x4.
        /// </summary>
        /// <returns></returns>
        public Matrix4x4 ReadMatrix4x4Unpacked()
        {
            Matrix4x4 result = new()
            {
                m00 = ReadSingleUnpacked(),
                m01 = ReadSingleUnpacked(),
                m02 = ReadSingleUnpacked(),
                m03 = ReadSingleUnpacked(),
                m10 = ReadSingleUnpacked(),
                m11 = ReadSingleUnpacked(),
                m12 = ReadSingleUnpacked(),
                m13 = ReadSingleUnpacked(),
                m20 = ReadSingleUnpacked(),
                m21 = ReadSingleUnpacked(),
                m22 = ReadSingleUnpacked(),
                m23 = ReadSingleUnpacked(),
                m30 = ReadSingleUnpacked(),
                m31 = ReadSingleUnpacked(),
                m32 = ReadSingleUnpacked(),
                m33 = ReadSingleUnpacked()
            };

            return result;
        }

        /// <summary>
        /// Reads a Matrix4x4.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Matrix4x4 ReadMatrix4x4() => ReadMatrix4x4Unpacked();

        /// <summary>
        /// Creates a new byte array and reads bytes into it.
        /// </summary>
        /// <param name = "count"></param>
        /// <returns></returns>
        public byte[] ReadUInt8ArrayAllocated(int count)
        {
            if (count < 0)
            {
                NetworkManager.Log($"Bytes count cannot be less than 0.");
                // Purge renaming and return default.
                Position += Remaining;
                return null;
            }

            if (IsPossibleAllocationAttack(count, movePositionOnTrue: true))
                return null;

            byte[] bytes = new byte[count];
            ReadUInt8Array(ref bytes, count);
            return bytes;
        }

        /// <summary>
        /// Reads a Guid.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Guid ReadGuid()
        {
            byte[] buffer = _guidBuffer;
            ReadUInt8Array(ref buffer, 16);
            return new(buffer);
        }

        /// <summary>
        /// Reads a tick without packing.
        /// </summary>
        public uint ReadTickUnpacked() => ReadUInt32Unpacked();

        /// <summary>
        /// Reads a GameObject.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public GameObject ReadGameObject()
        {
            byte writtenType = ReadUInt8Unpacked();

            GameObject result;
            // Do nothing for 0, as it indicates null.
            if (writtenType == 0)
            {
                result = null;
            }
            // 1 indicates a networkObject.
            else if (writtenType == 1)
            {
                NetworkObject nob = ReadNetworkObject();
                result = nob == null ? null : nob.gameObject;
            }
            // 2 indicates a networkBehaviour.
            else if (writtenType == 2)
            {
                NetworkBehaviour nb = ReadNetworkBehaviour();
                result = nb == null ? null : nb.gameObject;
            }
            else
            {
                result = null;
                NetworkManager.LogError($"Unhandled ReadGameObject type of {writtenType}.");
            }

            return result;
        }

        /// <summary>
        /// Reads a Transform.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public Transform ReadTransform()
        {
            NetworkObject nob = ReadNetworkObject();
            return nob == null ? null : nob.transform;
        }

        /// <summary>
        /// Reads a NetworkObject.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public NetworkObject ReadNetworkObject() => ReadNetworkObject(out _);

        /// <summary>
        /// Reads a NetworkObject.
        /// </summary>
        /// <returns></returns>
        public NetworkObject ReadNetworkObject(bool logException) => ReadNetworkObject(out _, null, logException);

        /// <summary>
        /// Reads a NetworkObject.
        /// </summary>
        /// <param name = "readSpawningObjects">Objects which have been read to be spawned this tick, but may not have spawned yet.</param>
        /// <returns></returns>
        public NetworkObject ReadNetworkObject(out int objectOrPrefabId, HashSet<int> readSpawningObjects = null, bool logException = true)
        {
            #if DEVELOPMENT
            LastNetworkBehaviour = null;
            #endif
            objectOrPrefabId = ReadNetworkObjectId();

            bool isSpawned;
            /* UNSET indicates that the object
             * is null or no PrefabId is set.
             * PrefabIds are set in Awake within
             * the NetworkManager so that should
             * never happen so long as nob isn't null. */
            if (objectOrPrefabId == NetworkObject.UNSET_OBJECTID_VALUE)
                return null;

            isSpawned = ReadBoolean();

            bool isServer = NetworkManager.ServerManager.Started;
            bool isClient = NetworkManager.ClientManager.Started;

            NetworkObject result;
            // Is spawned.
            if (isSpawned)
            {
                result = null;
                /* Try to get the object client side first if client
                 * is running. When acting as a host generally the object
                 * will be available in the server and client list
                 * but there can be occasions where the server side
                 * deinitializes the object, making it unavailable, while
                 * it is still available in the client side. Since FishNet doesn't
                 * use a fake host connection like some lesser solutions the client
                 * has to always be treated as it's own entity. */
                if (isClient)
                    NetworkManager.ClientManager.Objects.Spawned.TryGetValueIL2CPP(objectOrPrefabId, out result);
                // If not found on client and server is running then try server.
                if (result == null && isServer)
                    NetworkManager.ServerManager.Objects.Spawned.TryGetValueIL2CPP(objectOrPrefabId, out result);

                if (result == null && !isServer)
                {
                    if (logException && (readSpawningObjects == null || !readSpawningObjects.Contains(objectOrPrefabId)))
                        NetworkManager.LogWarning($"Spawned NetworkObject was expected to exist but does not for Id {objectOrPrefabId}. This may occur if you sent a NetworkObject reference which does not exist, be it destroyed or if the client does not have visibility.");
                }
            }
            // Not spawned.
            else
            {
                // Only look up asServer if not client, otherwise use client.
                bool asServer = !isClient;
                // Look up prefab.
                result = NetworkManager.GetPrefab(objectOrPrefabId, asServer);
            }

            #if DEVELOPMENT
            LastNetworkObject = result;
            #endif
            return result;
        }

        /// <summary>
        /// Reads a NetworkObjectId and nothing else.
        /// </summary>
        /// <returns></returns>
        public int ReadNetworkObjectId() => (int)ReadSignedPackedWhole();

        /// <summary>
        /// Reads the Id for a NetworkObject and outputs spawn settings.
        /// </summary>
        /// <returns></returns>
        internal int ReadNetworkObjectForSpawn(out int initializeOrder, out ushort collectionid)
        {
            int objectId = ReadNetworkObjectId();
            collectionid = ReadUInt16();
            initializeOrder = ReadInt32();

            return objectId;
        }

        /// <summary>
        /// Reads the Id for a NetworkObject and outputs despawn settings.
        /// </summary>
        /// <returns></returns>
        internal int ReadNetworkObjectForDespawn(out DespawnType dt)
        {
            int objectId = ReadNetworkObjectId();
            dt = (DespawnType)ReadUInt8Unpacked();
            return objectId;
        }

        /// <summary>
        /// Reads a NetworkBehaviourId and ObjectId.
        /// </summary>
        /// <returns></returns>
        internal byte ReadNetworkBehaviourId(out int objectId)
        {
            objectId = ReadNetworkObjectId();
            if (objectId != NetworkObject.UNSET_OBJECTID_VALUE)
                return ReadUInt8Unpacked();
            else
                return 0;
        }

        /// <summary>
        /// Reads a NetworkBehaviour.
        /// </summary>
        /// <param name = "readSpawningObjects">Objects which have been read to be spawned this tick, but may not have spawned yet.</param>
        /// <returns></returns>
        public NetworkBehaviour ReadNetworkBehaviour(out int objectId, out byte componentIndex, HashSet<int> readSpawningObjects = null, bool logException = true)
        {
            NetworkObject nob = ReadNetworkObject(out objectId, readSpawningObjects, logException);
            componentIndex = ReadUInt8Unpacked();

            NetworkBehaviour result;
            if (nob == null)
            {
                result = null;
            }
            else
            {
                if (componentIndex >= nob.NetworkBehaviours.Count)
                {
                    NetworkManager.LogError($"ComponentIndex of {componentIndex} is out of bounds on {nob.gameObject.name} [id {nob.ObjectId}]. This may occur if you have modified your gameObject/prefab without saving it, or the scene.");
                    result = null;
                }
                else
                {
                    result = nob.NetworkBehaviours[componentIndex];
                }
            }

            #if DEVELOPMENT
            LastNetworkBehaviour = result;
            #endif
            return result;
        }

        /// <summary>
        /// Reads a NetworkBehaviour.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public NetworkBehaviour ReadNetworkBehaviour()
        {
            return ReadNetworkBehaviour(out _, out _);
        }

        public NetworkBehaviour ReadNetworkBehaviour(bool logException)
        {
            return ReadNetworkBehaviour(out _, out _, null, logException);
        }

        /// <summary>
        /// Reads a NetworkBehaviourId.
        /// </summary>
        public byte ReadNetworkBehaviourId() => ReadUInt8Unpacked();

        /// <summary>
        /// Reads a DateTime.
        /// </summary>
        /// <param name = "dt"></param>
        [DefaultReader]
        public DateTime ReadDateTime()
        {
            long value = (long)ReadSignedPackedWhole();
            DateTime result = DateTime.FromBinary(value);
            return result;
        }

        /// <summary>
        /// Reads a transport channel.
        /// </summary>
        /// <param name = "channel"></param>
        [DefaultReader]
        public Channel ReadChannel()
        {
            return (Channel)ReadUInt8Unpacked();
        }

        /// <summary>
        /// Reads the Id for a NetworkConnection.
        /// </summary>
        /// <returns></returns>
        public int ReadNetworkConnectionId() => (int)ReadSignedPackedWhole();

        /// <summary>
        /// Reads a LayerMask.
        /// </summary>
        /// <returns></returns>
        [DefaultReader]
        public LayerMask ReadLayerMask()
        {
            int layerValue = (int)ReadSignedPackedWhole();
            return (LayerMask)layerValue;
        }

        /// <summary>
        /// Reads a NetworkConnection.
        /// </summary>
        /// <param name = "conn"></param>
        [DefaultReader]
        public NetworkConnection ReadNetworkConnection()
        {
            int value = ReadNetworkConnectionId();
            if (value == NetworkConnection.UNSET_CLIENTID_VALUE)
                return NetworkManager.EmptyConnection;

            NetworkConnection result;

            // Prefer server.
            if (NetworkManager.IsServerStarted)
            {
                if (NetworkManager.ServerManager.Clients.TryGetValueIL2CPP(value, out result))
                {
                    return result;
                }
                // If also client then try client side data.
                else if (NetworkManager.IsClientStarted)
                {
                    // If found in client collection then return.
                    if (NetworkManager.ClientManager.Clients.TryGetValueIL2CPP(value, out result))
                        return result;
                    /* Otherwise make a new instance.
                     * We do not know if this is for the server or client so
                     * initialize it either way. Connections rarely come through
                     * without being in server/client side collection. */

                    return new(NetworkManager, value, -1, true);
                }

                // Only server and not found.
                NetworkManager.LogWarning($"Unable to find connection for read Id {value}. An empty connection will be returned.");
                return NetworkManager.EmptyConnection;
            }

            // Try client side, will only be able to fetch against local connection.
            // If value is self then return self.
            if (value == NetworkManager.ClientManager.Connection.ClientId)
                return NetworkManager.ClientManager.Connection;
            // Try client side dictionary.
            if (NetworkManager.ClientManager.Clients.TryGetValueIL2CPP(value, out result))
                return result;
            /* Otherwise make a new instance.
             * We do not know if this is for the server or client so
             * initialize it either way. Connections rarely come through
             * without being in server/client side collection. */
            return new(NetworkManager, value, -1, true);
        }

        /// <summary>
        /// Reads TransformProperties.
        /// </summary>
        [DefaultReader]
        public TransformProperties ReadTransformProperties()
        {
            Vector3 position = ReadVector3();
            Quaternion rotation = ReadQuaternion32();
            Vector3 scale = ReadVector3();

            return new(position, rotation, scale);
        }

        /// <summary>
        /// Checks if the size could possibly be an allocation attack.
        /// </summary>
        /// <param name="movePositionOnTrue">True when on possible attacks to move the Position to the end of the reader to prevent further reads.</param>
        private bool IsPossibleAllocationAttack(int size, bool movePositionOnTrue)
        {
            /* Types which do not have set sizes cannot be determined
             * by size. It is however safe to assume that if a count(or size)
             * exceeds remaining bytes then an attack is possible. This is
             * true because at least 1 byte must be written to indicate null, which
             * means even a collection of 50 null entries would still use at least
             * 1 byte.
             *
             * For types that do not have set sizes the collections are not
             * reserved in memory, resize collections as needed rather than the new
             * keyword. This trades minor GC during possible resizes but offsets possible
             * attacks. */

            bool isPossibleAttack = false;

            /* Log is used over LogError to prevent logging
             * spam on the server. Generally server builds only
             * write error logs. */
            
            /* Possible attacks. Impossible size, or size indicates
             * more elements in collection or more bytes needed
             * than what bytes are available. */
            if (size < 0)
            {
                NetworkManager.Log($"Size of {size} is invalid.");
                isPossibleAttack = true;
            }
            else if (size > Remaining)
            {
                NetworkManager.Log($"Read size of {size} is larger than remaining data of {Remaining}.");
                isPossibleAttack = true;
            }

            if (isPossibleAttack && movePositionOnTrue)
                Position += Remaining;
            
            return isPossibleAttack;
        }

        /// <summary>
        /// Reads a state update packet.
        /// </summary>
        /// <param name = "tick"></param>
        internal void ReadStateUpdatePacket(out uint clientTick)
        {
            clientTick = ReadTickUnpacked();
        }

        #region Packed readers.
        /// <summary>
        /// ZigZag decode an integer. Move the sign bit back to the left.
        /// </summary>
        public ulong ZigZagDecode(ulong value)
        {
            ulong sign = value << 63;
            if (sign > 0)
                return ~(value >> 1) | sign;
            return value >> 1;
        }

        /// <summary>
        /// Reads a packed whole number and applies zigzag decoding.
        /// </summary>
        public long ReadSignedPackedWhole() => (long)ZigZagDecode(ReadUnsignedPackedWhole());

        /// <summary>
        /// Reads a packed whole number.
        /// </summary>
        public ulong ReadUnsignedPackedWhole()
        {
            int shift = 0;
            ulong value = 0;
            /* Maximum number of bytes for ulong.
             * Prevents endless loop. Should not be neccessary but is a nice precaution. */
            int maximumIterations = 10;
            int iterations = 0;
            int bufferLength = GetBuffer().Length;


            while (iterations < maximumIterations)
            {
                if (Position >= bufferLength)
                {
                    NetworkManager.LogError($"Read position of {Position} is beyond reader's buffer length of {bufferLength}.");
                    return 0;
                }

                byte currentByte = _buffer[Position++];
                value |= (ulong)(currentByte & 0x7F) << shift;

                if ((currentByte & 0x80) == 0)
                    break;

                shift += 7;
                iterations++;
            }

            return value;
        }
        #endregion

        #region Generators.
        /// <summary>
        /// Reads a reconcile.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        internal T ReadReconcile<T>() => Read<T>();

        /// <summary>
        /// Reads a replicate along with it's past replicates into a collection.
        /// </summary>
        internal List<ReplicateDataContainer<T>> ReadReplicate<T>(uint tick) where T : IReplicateData, new()
        {
            List<ReplicateDataContainer<T>> collection = CollectionCaches<ReplicateDataContainer<T>>.RetrieveList();

            // Number of entries written.
            int count = (int)ReadUInt8Unpacked();
            if (count <= 0)
            {
                NetworkManager.Log($"Replicate count cannot be 0 or less.");
                // Purge renaming and return default.
                Position += Remaining;
                return collection;
            }
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

            for (int i = 0; i < count; i++)
            {
                ReplicateDataContainer<T> value = ReadReplicateData<T>(tick + (uint)i);
                // Assign to collection.
                collection.Add(value);
            }

            return collection;
        }

        /// <summary>
        /// Reads a ReplicateData and applies tick and channel.
        /// </summary>
        private ReplicateDataContainer<T> ReadReplicateData<T>(uint tick) where T : IReplicateData, new()
        {
            T data = Read<T>();
            Channel c = ReadChannel();
            return new(data, c, tick, isCreated: true);
        }

        /// <summary>
        /// Reads a collection using a collection from caches.
        /// </summary>
        public Dictionary<TKey, TValue> ReadDictionary<TKey, TValue>()
        {
            int count = (int)ReadSignedPackedWhole();

            // Null collection.
            if (count == Writer.UNSET_COLLECTION_SIZE_VALUE)
                return null;

            Dictionary<TKey, TValue> collection = CollectionCaches<TKey, TValue>.RetrieveDictionary();
            ReadDictionary<TKey, TValue>(count, collection);

            return collection;
        }

        /// <summary>
        /// Reads a collection.
        /// </summary>
        [Obsolete("Use ReadDictionary.")]
        public Dictionary<TKey, TValue> ReadDictionaryAllocated<TKey, TValue>() => ReadDictionary<TKey, TValue>();

        /// <summary>
        /// Reads into collection and returns item count read.
        /// </summary>
        /// <param name = "collection"></param>
        /// <param name = "allowNullification">True to allow the referenced collection to be nullified when receiving a null collection read.</param>
        /// <returns>Number of values read into the collection. UNSET is returned if the collection were read as null.</returns>
        public int ReadDictionary<TKey, TValue>(ref Dictionary<TKey, TValue> collection, bool allowNullification = false)
        {
            int count = (int)ReadSignedPackedWhole();

            // Null collection.
            if (count == Writer.UNSET_COLLECTION_SIZE_VALUE)
            {
                if (allowNullification)
                    collection = null;

                return count;
            }

            ReadDictionary<TKey, TValue>(count, collection);

            return count;
        }

        /// <summary>
        /// Reads into a collection.
        /// </summary>
        private void ReadDictionary<TKey, TValue>(int count, Dictionary<TKey, TValue> collection)
        {
            if (IsPossibleAllocationAttack(count, movePositionOnTrue: true))
                return;

            //See IsPossibleAllocationAttack for why this is not initialized with count.
            if (collection == null)
                collection = new();
            else
                collection.Clear();

            for (int i = 0; i < count; i++)
            {
                TKey key = Read<TKey>();
                TValue value = Read<TValue>();
                collection.Add(key, value);
            }
        }

        /// <summary>
        /// Reads a collection using a collection from caches.
        /// </summary>
        public List<T> ReadList<T>()
        {
            int count = (int)ReadSignedPackedWhole();

            // Null collection.
            if (count == Writer.UNSET_COLLECTION_SIZE_VALUE)
                return null;

            List<T> result = CollectionCaches<T>.RetrieveList();
            ReadList(count, ref result);

            return result;
        }

        /// <summary>
        /// Reads a collection with allocations.
        /// </summary>
        [Obsolete("Use ReadList.")] // Remove in V5
        public List<T> ReadListAllocated<T>() => ReadList<T>();

        /// <summary>
        /// Reads into collection and returns item count read.
        /// </summary>
        /// <param name = "collection"></param>
        /// <param name = "allowNullification">True to allow the referenced collection to be nullified when receiving a null collection read.</param>
        /// <returns>Number of values read into the collection. UNSET is returned if the collection were read as null.</returns>
        public int ReadList<T>(ref List<T> collection, bool allowNullification = false)
        {
            int count = (int)ReadSignedPackedWhole();

            // Null collection.
            if (count == Writer.UNSET_COLLECTION_SIZE_VALUE)
            {
                if (allowNullification)
                    collection = null;

                return count;
            }

            ReadList<T>(count, ref collection);

            return count;
        }

        /// <summary>
        /// Reads into a collection.
        /// </summary>
        private void ReadList<T>(int count, ref List<T> collection)
        {
            if (IsPossibleAllocationAttack(count, movePositionOnTrue: true))
                return;

            //See IsPossibleAllocationAttack for why this is not initialized with count.
            if (collection == null)
                collection = new();
            else
                collection.Clear();

            for (int i = 0; i < count; i++)
                collection.Add(Read<T>());
        }

        /// <summary>
        /// Reads a collection using a collection from caches.
        /// </summary>
        public HashSet<T> ReadHashSet<T>()
        {
            int count = (int)ReadSignedPackedWhole();

            // Null collection.
            if (count == Writer.UNSET_COLLECTION_SIZE_VALUE)
                return null;

            HashSet<T> result = CollectionCaches<T>.RetrieveHashSet();
            ReadHashSet(count, ref result);

            return result;
        }

        /// <summary>
        /// Reads into collection and returns item count read.
        /// </summary>
        /// <param name = "collection"></param>
        /// <param name = "allowNullification">True to allow the referenced collection to be nullified when receiving a null collection read.</param>
        /// <returns>Number of values read into the collection. UNSET is returned if the collection were read as null.</returns>
        public int HashSet<T>(ref HashSet<T> collection, bool allowNullification = false)
        {
            int count = (int)ReadSignedPackedWhole();

            // Null collection.
            if (count == Writer.UNSET_COLLECTION_SIZE_VALUE)
            {
                if (allowNullification)
                    collection = null;

                return count;
            }

            ReadHashSet<T>(count, ref collection);

            return count;
        }

        /// <summary>
        /// Reads into a collection.
        /// </summary>
        private void ReadHashSet<T>(int count, ref HashSet<T> collection)
        {
            if (IsPossibleAllocationAttack(count, movePositionOnTrue: true))
                return;

            //See IsPossibleAllocationAttack for why this is not initialized with count.
            if (collection == null)
                collection = new();
            else
                collection.Clear();
            
            for (int i = 0; i < count; i++)
                collection.Add(Read<T>());
        }

        /// <summary>
        /// Reads a collection.
        /// </summary>
        public T[] ReadArrayAllocated<T>()
        {
            T[] result = null;
            ReadArray(ref result);

            return result;
        }

        /// <summary>
        /// Reads into collection and returns amount read.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <param name = "collection"></param>
        /// <returns></returns>
        public int ReadArray<T>(ref T[] collection)
        {
            int count = (int)ReadSignedPackedWhole();

            if (count == Writer.UNSET_COLLECTION_SIZE_VALUE)
                return 0;

            if (count == 0)
            {
                if (collection == null)
                    collection = new T[0];

                return 0;
            }

            if (IsPossibleAllocationAttack(count, movePositionOnTrue: true))
                return 0;

            /* Read to a list then move the values into an array.
             * This does cost some extra overhead but this method
             * is expected to rarely be used, and this guards against
             * a memory allocation attack. */

            List<T> listResults = CollectionCaches<T>.RetrieveList();
            for (int i = 0; i < count; i++)
                listResults.Add(Read<T>());

            /* If here then reader was not overflowed; it is
             * safe to allocate the array and copy. */
            if (collection == null)
                collection = new T[count];
            else if (collection.Length < count)
                Array.Resize(ref collection, count);

            for (int i = 0; i < count; i++)
                collection[i] = listResults[i];

            CollectionCaches<T>.Store(listResults);

            return count;
        }

        /// <summary>
        /// Reads any supported type as packed.
        /// </summary>
        public T Read<T>()
        {
            Func<Reader, T> del = GenericReader<T>.Read;

            if (del == null)
            {
                NetworkManager.LogError($"Read method not found for {typeof(T).FullName}. Use a supported type or create a custom serializer.");
                return default;
            }
            else
            {
                return del.Invoke(this);
            }
        }
        #endregion
    }
}