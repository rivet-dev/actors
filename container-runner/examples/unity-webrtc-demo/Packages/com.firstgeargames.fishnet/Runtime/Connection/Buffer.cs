using FishNet.Managing;
using FishNet.Managing.Logging;
using FishNet.Managing.Transporting;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Utility.Performance;
using System;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Connection
{
    /// <summary>
    /// A byte buffer that automatically resizes.
    /// </summary>
    internal class ByteBuffer
    {
        /// <summary>
        /// How many more bytes may fit into the buffer.
        /// </summary>
        internal int Remaining => Size - Length;
        /// <summary>
        /// Buffer data.
        /// </summary>
        internal byte[] Data { get; private set; }
        /// <summary>
        /// How many bytes currently into Data. This will include the reserve.
        /// </summary>
        internal int Length { get; private set; }
        /// <summary>
        /// Size of the buffer. Data.Length may exceed this value as it uses a pooled array.
        /// </summary>
        internal int Size { get; private set; }
        /// <summary>
        /// True if data has been written.
        /// </summary>
        internal bool HasData { get; private set; }
        /// <summary>
        /// Bytes to reserve when resetting.
        /// </summary>
        private int _reserve;

        internal ByteBuffer(int size, int reserve = 0)
        {
            Data = ByteArrayPool.Retrieve(size);
            Size = size;
            _reserve = reserve;
            Reset();
        }

        public void Dispose()
        {
            if (Data != null)
                ByteArrayPool.Store(Data);
            Data = null;
            Reset();
        }

        /// <summary>
        /// Resets instance without clearing Data.
        /// </summary>
        internal void Reset()
        {
            Length = _reserve;
            HasData = false;
        }

        /// <summary>
        /// Copies segments without error checking, including tick for the first time data is added.
        /// </summary>
        /// <param name = "segment"></param>
        internal void CopySegment(uint tick, ArraySegment<byte> segment)
        {
            /* If data has not been written to buffer yet
             * then write tick to the start. */
            if (!HasData)
            {
                int pos = 0;
                Writer.WriteUInt32Unpacked(Data, tick, ref pos);
            }

            Buffer.BlockCopy(segment.Array, segment.Offset, Data, Length, segment.Count);
            Length += segment.Count;
            HasData = true;
        }

        /// <summary>
        /// Copies segments without error checking.
        /// </summary>
        /// <param name = "segment"></param>
        internal void CopySegment(ArraySegment<byte> segment)
        {
            Buffer.BlockCopy(segment.Array, segment.Offset, Data, Length, segment.Count);
            Length += segment.Count;
            HasData = true;
        }
    }

    internal class PacketBundle
    {
        /// <summary>
        /// True if data has been written.
        /// </summary>
        internal bool HasData => _buffers[0].HasData || (!_isSendLastBundle && _sendLastBundle.HasData);
        /// <summary>
        /// All buffers written. Collection is not cleared when reset but rather the index in which to write is.
        /// </summary>
        private List<ByteBuffer> _buffers = new();
        /// <summary>
        /// Buffer which is being written to.
        /// </summary>
        private int _bufferIndex;
        /// <summary>
        /// Maximum size packet the transport can handle.
        /// </summary>
        private int _maximumTransportUnit;
        /// <summary>
        /// Number of buffers written to. Will return 0 if nothing has been written.
        /// </summary>
        public int WrittenBuffers => !HasData ? 0 : _bufferIndex + 1;
        /// <summary>
        /// Number of bytes to reserve at the beginning of each buffer.
        /// </summary>
        private int _reserve;
        /// <summary>
        /// NetworkManager this is for.
        /// </summary>
        private NetworkManager _networkManager;
        /// <summary>
        /// Packet bundle to use for last enqueued data.
        /// </summary>
        private PacketBundle _sendLastBundle;
        /// <summary>
        /// True if being used as an sendLast bundle.
        /// </summary>
        private bool _isSendLastBundle;

        internal PacketBundle(NetworkManager manager, int mtu, int reserve = 0, DataOrderType orderType = DataOrderType.Default)
        {
            _isSendLastBundle = orderType == DataOrderType.Last;
            // If this is not the send last packetBundle then make a new one.
            if (!_isSendLastBundle)
                _sendLastBundle = new(manager, mtu, reserve, DataOrderType.Last);

            _networkManager = manager;
            _maximumTransportUnit = mtu;
            /* Allow bytes for the tick.
             * Modify reserve after making sendLast bundle
             * so that the wrong reserve is not passed into
             * the sendLast bundle. */
            reserve += TransportManager.UNPACKED_TICK_LENGTH;
            _reserve = reserve;
            // Add buffer requires the right reserve so call after setting.
            AddBuffer();

            Reset(false);
        }

        public void Dispose()
        {
            for (int i = 0; i < _buffers.Count; i++)
                _buffers[i].Dispose();

            _sendLastBundle?.Dispose();
        }

        /// <summary>
        /// Adds a buffer using current settings.
        /// </summary>
        private ByteBuffer AddBuffer()
        {
            ByteBuffer ba = new(_maximumTransportUnit, _reserve);
            _buffers.Add(ba);
            return ba;
        }

        /// <summary>
        /// Resets using current settings.
        /// </summary>
        internal void Reset(bool resetSendLast)
        {
            _bufferIndex = 0;

            for (int i = 0; i < _buffers.Count; i++)
                _buffers[i].Reset();

            if (resetSendLast)
                _sendLastBundle.Reset(false);
        }

        /// <summary>
        /// Writes a segment to this packet bundle using the current WriteIndex.
        /// </summary>
        /// <param name = "forceNewBuffer">True to force data into a new buffer.</param>
        internal void Write(ArraySegment<byte> segment, bool forceNewBuffer = false, DataOrderType orderType = DataOrderType.Default)
        {
            /* If not the send last bundle and to send data last
             * then send using the send last bundle. */
            if (!_isSendLastBundle && orderType == DataOrderType.Last)
            {
                _sendLastBundle.Write(segment, forceNewBuffer, orderType);
                return;
            }

            // Nothing to be written.
            if (segment.Count == 0)
                return;

            /* If the segment count is larger than the mtu then
             * something went wrong. Nothing should call this method
             * directly except the TransportManager, which will automatically
             * split packets that exceed MTU into reliable ordered. */
            if (segment.Count > _maximumTransportUnit)
            {
                _networkManager.LogError($"Segment is length of {segment.Count} while MTU is {_maximumTransportUnit}. Packet was not split properly and will not be sent.");
                return;
            }

            /* A buffer is added during initialization, so one
             * will always be present. */
            ByteBuffer ba = _buffers[_bufferIndex];

            //True if nothing has been written to the buffer.
            bool isBufferEmpty = ba.Length == _reserve;

            /* If nothing has been written yet there
             * is no reason to force a new buffer. */
            if (isBufferEmpty)
            {
                forceNewBuffer = false;
            }
            else
            {
                if (!forceNewBuffer)
                {
                    bool canBufferHoldSegmentCount = segment.Count <= ba.Remaining;

                    if (!canBufferHoldSegmentCount)
                        forceNewBuffer = true;
                }
            }

            if (forceNewBuffer)
            {
                _bufferIndex++;

                // If need to make a new buffer then do so.
                if (_buffers.Count <= _bufferIndex)
                {
                    ba = AddBuffer();
                }
                else
                {
                    ba = _buffers[_bufferIndex];
                    ba.Reset();
                }
            }

            uint tick = _networkManager.TimeManager.LocalTick;
            ba.CopySegment(tick, segment);
        }

        /// <summary>
        /// Returns the packetBundle for send last.
        /// </summary>
        /// <returns></returns>
        internal PacketBundle GetSendLastBundle() => _sendLastBundle;

        /// <summary>
        /// Gets a buffer for the specified index. Returns true and outputs the buffer if it was successfully found.
        /// </summary>
        /// <param name = "index">Index of the buffer to retrieve.</param>
        /// <param name = "bb">Buffer retrieved from the list. Null if the specified buffer was not found.</param>
        internal bool GetBuffer(int index, out ByteBuffer bb)
        {
            bb = null;

            if (index >= _buffers.Count || index < 0)
            {
                _networkManager.LogError($"Index of {index} is out of bounds. There are {_buffers.Count} available.");
                return false;
            }
            if (index > _bufferIndex)
            {
                _networkManager.LogError($"Index of {index} exceeds the number of written buffers. There are {WrittenBuffers} written buffers.");
                return false;
            }

            bb = _buffers[index];
            return bb.HasData;
        }

        /// <summary>
        /// Returns a PacketBundle for a channel. ResetPackets must be called afterwards.
        /// </summary>
        /// <param name = "channel"></param>
        /// <returns>True if PacketBundle is valid on the index and contains data.</returns>
        internal static bool GetPacketBundle(int channel, List<PacketBundle> bundles, out PacketBundle mtuBuffer)
        {
            // Out of bounds.
            if (channel >= bundles.Count)
            {
                mtuBuffer = null;
                return false;
            }

            mtuBuffer = bundles[channel];
            return mtuBuffer.HasData;
        }
    }
}