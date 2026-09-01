using FishNet.Connection;
using FishNet.Transporting;
using FishNet.Utility.Performance;
using System;
using System.Collections.Generic;
using UnityEngine;

// Thanks to TiToMoskito originally creating this as a Transport.
// https://github.com/TiToMoskito/FishyLatency
namespace FishNet.Managing.Transporting
{
    [Serializable]
    public class LatencySimulator
    {
        #region Types.
        /// <summary>
        /// A message affected by latency.
        /// </summary>
        private struct Message
        {
            public readonly int ConnectionId;
            public readonly byte[] Data;
            public readonly int Length;
            public readonly float SendTime;

            public Message(int connectionId, ArraySegment<byte> segment, float latency)
            {
                ConnectionId = connectionId;
                SendTime = Time.unscaledTime + latency;
                Length = segment.Count;
                Data = ByteArrayPool.Retrieve(Length);
                Buffer.BlockCopy(segment.Array, segment.Offset, Data, 0, Length);
            }

            public ArraySegment<byte> GetSegment()
            {
                return new(Data, 0, Length);
            }
        }
        #endregion

        #region Internal.
        /// <summary>
        /// True if latency can be simulated.
        /// </summary>
        internal bool CanSimulate => GetEnabled() && (GetLatency() > 0 || GetPacketLost() > 0 || GetOutOfOrder() > 0);
        #endregion

        #region Serialized
        [Header("Settings")]
        /// <summary>
        /// 
        /// </summary>
        [Tooltip("True if latency simulator is enabled.")]
        [SerializeField]
        private bool _enabled;

        /// <summary>
        /// Gets the enabled value of simulator.
        /// </summary>
        public bool GetEnabled() => _enabled;

        /// <summary>
        /// Sets the enabled value of simulator.
        /// </summary>
        /// <param name = "value">New value.</param>
        public void SetEnabled(bool value)
        {
            if (value == _enabled)
                return;

            _enabled = value;
            Reset();
        }

        /// <summary>
        /// </summary>
        [Tooltip("True to add latency on clientHost as well.")]
        [SerializeField]
        private bool _simulateHost = true;
        /// <summary>
        /// Milliseconds to add between packets. When acting as host this value will be doubled. Added latency will be a minimum of tick rate.
        /// </summary>
        [Tooltip("Milliseconds to add between packets. When acting as host this value will be doubled. Added latency will be a minimum of tick rate.")]
        [Range(0, 60000)]
        [SerializeField]
        private long _latency = 0;

        /// <summary>
        /// Gets the latency value.
        /// </summary>
        /// <returns></returns>
        public long GetLatency() => _latency;

        /// <summary>
        /// Sets a new latency value.
        /// </summary>
        /// <param name = "value">Latency as milliseconds.</param>
        public void SetLatency(long value) => _latency = value;

        [Header("Unreliable")]
        /// <summary>
        /// Percentage of unreliable packets which should arrive out of order.
        /// </summary>
        [Tooltip("Percentage of unreliable packets which should arrive out of order.")]
        [Range(0f, 1f)]
        [SerializeField]
        private double _outOfOrder = 0;

        /// <summary>
        /// Out of order chance, 1f is a 100% chance to occur.
        /// </summary>
        /// <returns></returns>
        public double GetOutOfOrder() => _outOfOrder;

        /// <summary>
        /// Sets out of order chance. 1f is a 100% chance to occur.
        /// </summary>
        /// <param name = "value">New Value.</param>
        public void SetOutOfOrder(double value) => _outOfOrder = value;

        /// <summary>
        /// Percentage of packets which should drop.
        /// </summary>
        [Tooltip("Percentage of packets which should drop.")]
        [Range(0, 1)]
        [SerializeField]
        private double _packetLoss = 0;

        /// <summary>
        /// Gets packet loss chance. 1f is a 100% chance to occur.
        /// </summary>
        /// <returns></returns>
        public double GetPacketLost() => _packetLoss;

        /// <summary>
        /// Sets packet loss chance. 1f is a 100% chance to occur.
        /// </summary>
        /// <param name = "value">New Value.</param>
        public void SetPacketLoss(double value) => _packetLoss = value;
        #endregion

        #region Private
        /// <summary>
        /// Transport to send data on.
        /// </summary>
        private Transport _transport;
        /// <summary>
        /// Reliable messages to the server.
        /// </summary>
        private List<Message> _toServerReliable = new();
        /// <summary>
        /// Unreliable messages to the server.
        /// </summary>
        private List<Message> _toServerUnreliable = new();
        /// <summary>
        /// Reliable messages to clients.
        /// </summary>
        private List<Message> _toClientReliable = new();
        /// <summary>
        /// Unreliable messages to clients.
        /// </summary>
        private List<Message> _toClientUnreliable = new();
        /// <summary>
        /// NetworkManager for this instance.
        /// </summary>
        private NetworkManager _networkManager;
        /// <summary>
        /// Used to generate chances of latency.
        /// </summary>
        private readonly System.Random _random = new();
        #endregion

        #region Initialization and Unity
        public void Initialize(NetworkManager manager, Transport transport)
        {
            _networkManager = manager;
            _transport = transport;
        }
        #endregion

        /// <summary>
        /// Stops both client and server.
        /// </summary>
        public void Reset()
        {
            bool enabled = GetEnabled();
            if (_transport != null && enabled)
            {
                IterateAndStore(_toServerReliable);
                IterateAndStore(_toServerUnreliable);
                IterateAndStore(_toClientReliable);
                IterateAndStore(_toClientUnreliable);
            }

            void IterateAndStore(List<Message> messages)
            {
                foreach (Message m in messages)
                {
                    _transport.SendToServer((byte)Channel.Reliable, m.GetSegment());
                    ByteArrayPool.Store(m.Data);
                }
            }

            _toServerReliable.Clear();
            _toServerUnreliable.Clear();
            _toClientReliable.Clear();
            _toClientUnreliable.Clear();
        }

        /// <summary>
        /// Removes pending or held packets for a connection.
        /// </summary>
        /// <param name = "conn">Connection to remove pending packets for.</param>
        public void RemovePendingForConnection(int connectionId)
        {
            // If not enabled exit early to save work.
            if (!GetEnabled())
                return;

            RemoveFromCollection(_toServerUnreliable);
            RemoveFromCollection(_toServerUnreliable);
            RemoveFromCollection(_toClientReliable);
            RemoveFromCollection(_toClientUnreliable);

            void RemoveFromCollection(List<Message> c)
            {
                for (int i = 0; i < c.Count; i++)
                {
                    if (c[i].ConnectionId == connectionId)
                    {
                        c.RemoveAt(i);
                        i--;
                    }
                }
            }
        }

        #region Simulation
        /// <summary>
        /// Returns long latency as a float.
        /// </summary>
        /// <param name = "ms"></param>
        /// <returns></returns>
        private float GetLatencyAsFloat()
        {
            return (float)(_latency / 1000f);
        }

        /// <summary>
        /// Adds a packet for simulation.
        /// </summary>
        public void AddOutgoing(byte channelId, ArraySegment<byte> segment, bool toServer = true, int connectionId = -1)
        {
            /* If to not simulate for host see if this packet
             * should be sent normally. */
            if (!_simulateHost && _networkManager != null && _networkManager.IsHostStarted)
            {
                /* If going to the server and is host then
                 * it must be sent from clientHost. */
                if (toServer)
                {
                    _transport.SendToServer(channelId, segment);
                    return;
                }
                //Not to server, see if going to clientHost.
                else
                {
                    //If connId is the same as clientHost id.
                    if (_networkManager.ClientManager.Connection.ClientId == connectionId)
                    {
                        _transport.SendToClient(channelId, segment, connectionId);
                        return;
                    }
                }
            }

            List<Message> collection;
            Channel c = (Channel)channelId;

            if (toServer)
                collection = c == Channel.Reliable ? _toServerReliable : _toServerUnreliable;
            else
                collection = c == Channel.Reliable ? _toClientReliable : _toClientUnreliable;

            float latency = GetLatencyAsFloat();
            //If dropping check to add extra latency if reliable, or discard if not.
            if (DropPacket())
            {
                if (c == Channel.Reliable)
                {
                    latency += latency * 0.3f; //add extra for resend.
                }
                //If not reliable then return the segment array to pool.
                else
                {
                    return;
                }
            }

            Message msg = new(connectionId, segment, latency);
            int count = collection.Count;
            if (c == Channel.Unreliable && count > 0 && OutOfOrderPacket(c))
                collection.Insert(count - 1, msg);
            else
                collection.Add(msg);
        }

        /// <summary>
        /// Simulates pending outgoing packets.
        /// </summary>
        /// <param name = "asServer">True to send data from the local server to clients, false to send from the local client to server.
        public void IterateOutgoing(bool asServer)
        {
            if (_transport == null)
            {
                Reset();
                return;
            }

            if (asServer)
            {
                IterateCollection(_toClientReliable, Channel.Reliable);
                IterateCollection(_toClientUnreliable, Channel.Unreliable);
            }
            else
            {
                IterateCollection(_toServerReliable, Channel.Reliable);
                IterateCollection(_toServerUnreliable, Channel.Unreliable);
            }

            void IterateCollection(List<Message> collection, Channel channel)
            {
                byte cByte = (byte)channel;
                float unscaledTime = Time.unscaledTime;

                int count = collection.Count;
                int iterations = 0;
                for (int i = 0; i < count; i++)
                {
                    Message msg = collection[i];
                    //Not enough time has passed.
                    if (unscaledTime < msg.SendTime)
                        break;

                    if (asServer)
                        _transport.SendToClient(cByte, msg.GetSegment(), msg.ConnectionId);
                    else
                        _transport.SendToServer(cByte, msg.GetSegment());

                    iterations++;
                }

                if (iterations > 0)
                {
                    for (int i = 0; i < iterations; i++)
                        ByteArrayPool.Store(collection[i].Data);
                    collection.RemoveRange(0, iterations);
                }
            }

            _transport.IterateOutgoing(asServer);
        }

        /// <summary>
        /// Returns if a packet should drop.
        /// </summary>
        /// <returns></returns>
        private bool DropPacket()
        {
            return _packetLoss > 0d && _random.NextDouble() < _packetLoss;
        }

        /// <summary>
        /// Returns if a packet should be out of order.
        /// </summary>
        /// <param name = "c"></param>
        /// <returns></returns>
        private bool OutOfOrderPacket(Channel c)
        {
            if (c == Channel.Reliable)
                return false;

            return _outOfOrder > 0d && _random.NextDouble() < _outOfOrder;
        }
        #endregion
    }
}