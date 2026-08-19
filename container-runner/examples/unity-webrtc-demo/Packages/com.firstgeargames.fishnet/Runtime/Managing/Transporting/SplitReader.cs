using FishNet.Serializing;
using System;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Managing.Transporting
{
    internal class SplitReader : IResettable
    {
        
        #region Private.
        /// <summary>
        /// Writer containing the combined split packet.
        /// </summary>
        private readonly PooledWriter _writer = new();
        /// <summary>
        /// Expected number of splits.
        /// </summary>
        private int _expectedMessages;
        /// <summary>
        /// Number of splits received so far.
        /// </summary>
        private ushort _receivedMessages;
        /// <summary>
        /// The maximum allowed bytes which can be read. This acts as a guard against overflow.
        /// </summary>
        private uint _maximumClientBytes;
        /// <summary>
        /// NetworkManager for this.
        /// </summary>
        private NetworkManager _networkManager;
        /// <summary>
        /// True if the sender of the split packet is a client.
        /// </summary>
        /// <returns></returns>
        private bool _isSenderClient;
        #endregion

        public void Initialize(NetworkManager networkManager, uint maximumClientBytes, bool isSenderClient, int expectedMessages)
        {
            _networkManager = networkManager;
            _maximumClientBytes = maximumClientBytes;
            _isSenderClient = isSenderClient;
            _expectedMessages = expectedMessages;
            
            
            /* This is just a guess as to how large the end
             * message could be. If the writer is not the minimum
             * of this length then resize it. */
            int estimatedBufferSize = expectedMessages * 1500;
            if (_writer.Capacity < estimatedBufferSize)
                _writer.EnsureBufferCapacity(estimatedBufferSize);
        }

        /// <summary>
        /// Combines split data.
        /// </summary>
        internal bool Write(PooledReader reader)
        {
            if (_isSenderClient)
            {
                long totalBytes = _writer.Length + reader.Remaining;

                if (totalBytes > _maximumClientBytes)
                {
                    _networkManager.LogError($"A split packet of [{totalBytes}] exceeds the maximum allowed bytes of [{_maximumClientBytes}].");
                    return false;
                }
            }
            
            /* Empty remainder of reader into the writer.
             * It does not matter if parts of the reader
             * contain data added after the split because
             * once the split is fully combined the data
             * is parsed as though it came in as one message,
             * which is how data is normally read. */
            ArraySegment<byte> data = reader.ReadArraySegment(reader.Remaining);
            _writer.WriteArraySegment(data);

            
            _receivedMessages++;
            
            return true;
        }

        /// <summary>
        /// Returns if all split messages have been received.
        /// </summary>
        /// <returns></returns>
        internal bool TryGetFullMessage(out ArraySegment<byte> segment)
        {
            if (_receivedMessages < _expectedMessages)
            {
                segment = ArraySegment<byte>.Empty;
                return false;
            }

            segment = _writer.GetArraySegment();
            return true;
        }

        public void ResetState()
        {
            _writer.Clear();
            
            _expectedMessages = 0;
            _receivedMessages = 0;
            _maximumClientBytes = 0;

            _networkManager = null;
        }

        public void InitializeState() { }
    }
}