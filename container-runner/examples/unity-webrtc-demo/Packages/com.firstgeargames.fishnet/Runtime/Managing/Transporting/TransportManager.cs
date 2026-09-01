#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Connection;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Transporting.Multipass;
using System;
using System.Collections.Generic;
using FishNet.Managing.Statistic;
using GameKit.Dependencies.Utilities;
using UnityEngine;
using UnityEngine.Serialization;

namespace FishNet.Managing.Transporting
{
    /// <summary>
    /// Communicates with the Transport to send and receive data.
    /// </summary>
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Manager/TransportManager")]
    public sealed partial class TransportManager : MonoBehaviour
    {
        #region Types.
        private struct DisconnectingClient
        {
            public uint Tick;
            public NetworkConnection Connection;

            public DisconnectingClient(uint tick, NetworkConnection connection)
            {
                Tick = tick;
                Connection = connection;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Returns if an IntermediateLayer is in use.
        /// </summary>
        public bool HasIntermediateLayer => _intermediateLayer != null;
        /// <summary>
        /// Called before IterateOutgoing has started.
        /// </summary>
        internal event Action OnIterateOutgoingStart;
        /// <summary>
        /// Called after IterateOutgoing has completed.
        /// </summary>
        internal event Action OnIterateOutgoingEnd;
        /// <summary>
        /// Called before IterateIncoming has started. True for on server, false for on client.
        /// </summary>
        internal event Action<bool> OnIterateIncomingStart;
        /// <summary>
        /// Called after IterateIncoming has completed. True for on server, false for on client.
        /// </summary>
        internal event Action<bool> OnIterateIncomingEnd;
        /// <summary>
        /// The current Transport being used.
        /// </summary>
        [Tooltip("The current Transport being used.")]
        public Transport Transport;
        #endregion

        #region Serialized.
        /// <summary>
        /// The maximum amount of bytes of any combined packet that a client may send.  
        /// </summary>
        public uint MaximumClientPacketSize => _maximumClientPacketSize;
        [Tooltip("The maximum amount of bytes of any combined packet that a client may send.")]
        [SerializeField]
        private uint _maximumClientPacketSize = 20480;
        /// <summary>
        /// Layer used to modify data before it is sent or received.
        /// </summary>
        [Tooltip("Layer used to modify data before it is sent or received.")]
        [SerializeField]
        private IntermediateLayer _intermediateLayer;
        /// <summary>
        /// </summary>
        [Tooltip("Latency simulation settings.")]
        [SerializeField]
        private LatencySimulator _latencySimulator = new();
        /// <summary>
        /// Latency simulation settings.
        /// </summary>
        public LatencySimulator LatencySimulator
        {
            get
            {
                // Shouldn't ever be null unless the user nullifies it.
                if (_latencySimulator == null)
                    _latencySimulator = new();
                return _latencySimulator;
            }
        }
        #endregion

        #region Private.
        /// <summary>
        /// NetworkConnections on the server which have to send data to clients.
        /// </summary>
        private List<NetworkConnection> _dirtyToClients = new();
        /// <summary>
        /// PacketBundles to send to the server.
        /// </summary>
        private List<PacketBundle> _toServerBundles = new();
        /// <summary>
        /// NetworkManager handling this TransportManager.
        /// </summary>
        private NetworkManager _networkManager;
        /// <summary>
        /// Clients which are pending disconnects.
        /// </summary>
        private List<DisconnectingClient> _disconnectingClients = new();
        /// <summary>
        /// Lowest MTU of all transports for channels.
        /// </summary>
        private int[] _lowestMtus;
        /// <summary>
        /// Lowest MTU of all transports of all channels.
        /// </summary>
        private int _lowestMtu = 0;
        /// <summary>
        /// Custom amount to reserve on the MTU.
        /// </summary>
        private int _customMtuReserve = MINIMUM_MTU_RESERVE;
        /// <summary>
        /// </summary>
        private NetworkTrafficStatistics _networkTrafficStatistics;
        /// <summary>
        /// Maximum size which each segment of a split message can be.
        /// </summary>
        private int _maximumSplitPacketSegmentLength => GetLowestMTU(SPLIT_PACKET_CHANNELID) - SPLIT_PACKET_HEADER_LENGTH - UNPACKED_TICK_LENGTH;
        #endregion

        #region Consts.
        /// <summary>
        /// Number of bytes sent for PacketId.
        /// </summary>
        public const byte PACKETID_LENGTH = 2;
        /// <summary>
        /// Number of bytes sent for ObjectId.
        /// </summary>
        public const byte OBJECT_ID_LENGTH = 2;
        /// <summary>
        /// Number of bytes sent for ComponentIndex.
        /// </summary>
        public const byte COMPONENT_INDEX_LENGTH = 1;
        /// <summary>
        /// Number of bytes sent for Tick.
        /// </summary>
        public const byte UNPACKED_TICK_LENGTH = 4;
        /// <summary>
        /// Number of bytes sent for an unpacked size, such as a collection or array size.
        /// </summary>
        public const byte UNPACKED_SIZE_LENGTH = 4;
        /// <summary>
        /// Number of bytes sent to indicate split count.
        /// </summary>
        private const byte SPLIT_COUNT_LENGTH = 4;
        /// <summary>
        /// Number of bytes required for split data.
        /// </summary>
        public const byte SPLIT_PACKET_HEADER_LENGTH = PACKETID_LENGTH + SPLIT_COUNT_LENGTH;
        /// <summary>
        /// Number of channels supported.
        /// </summary>
        public const byte CHANNEL_COUNT = 2;
        /// <summary>
        /// MTU reserved for internal use.
        /// 1 byte is used to specify channel in packets for transports that do not include channel within their packet header. This is transport dependent.
        /// </summary>
        public const int MINIMUM_MTU_RESERVE = 1;
        /// <summary>
        /// Value to use when a MTU could not be found.
        /// </summary>
        public const int INVALID_MTU = -1;
        /// <summary>
        /// A split message was not required, the value can be sent normally.
        /// </summary>
        private const int SPLIT_NOT_REQUIRED_VALUE = 0;
        /// <summary>
        /// A message was sent split.
        /// </summary>
        private const int SPLIT_SENT_VALUE = 1;
        /// <summary>
        /// An error occurred while trying to split a message.
        /// </summary>
        private const int SPLIT_ERROR_VALUE = 2;
        /// <summary>
        /// ChannelId to use for split packets.
        /// </summary>
        private const byte SPLIT_PACKET_CHANNELID = (byte)Channel.Reliable;
        #endregion

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        internal void InitializeOnce_Internal(NetworkManager manager)
        {
            _networkManager = manager;
            TryAddDefaultTransport();
            Transport.Initialize(_networkManager, 0);
            SetLowestMTUs();
            InitializeToServerBundles();

            manager.StatisticsManager.TryGetNetworkTrafficStatistics(out _networkTrafficStatistics);

            manager.ServerManager.OnServerConnectionState += ServerManager_OnServerConnectionState;
            manager.ClientManager.OnClientConnectionState += ClientManager_OnClientConnectionState;

            if (_intermediateLayer != null)
                _intermediateLayer.InitializeOnce(this);
            #if DEVELOPMENT
            _latencySimulator.Initialize(manager, Transport);
            #endif
        }

        /// <summary>
        /// Sets the lowest MTU values.
        /// </summary>
        private void SetLowestMTUs()
        {
            // Already set.
            if (_lowestMtu != 0)
                return;

            /* At least one transport is required.
             * Try to add default. If a transport is already
             * specified the add method will just exit early. */
            TryAddDefaultTransport();

            int allLowest = int.MaxValue;
            // Cache lowest Mtus.
            _lowestMtus = new int[CHANNEL_COUNT];
            for (byte i = 0; i < CHANNEL_COUNT; i++)
            {
                int channelLowest = int.MaxValue;
                if (Transport is Multipass mp)
                {
                    foreach (Transport t in mp.Transports)
                    {
                        int mtu = t.GetMTU(i);
                        if (mtu != INVALID_MTU)
                            channelLowest = Mathf.Min(channelLowest, mtu);
                    }
                }
                else
                {
                    channelLowest = Transport.GetMTU(i);
                }

                _lowestMtus[i] = channelLowest;
                _lowestMtu = Mathf.Min(allLowest, channelLowest);
            }
        }

        /// <summary>
        /// Adds the default transport if a transport is not yet specified.
        /// </summary>
        private void TryAddDefaultTransport()
        {
            if (Transport == null && !gameObject.TryGetComponent(out Transport))
                Transport = gameObject.AddComponent<FishNet.Transporting.Tugboat.Tugboat>();
        }

        /// <summary>
        /// Called when the local connection state changes for the client.
        /// </summary>
        private void ClientManager_OnClientConnectionState(ClientConnectionStateArgs obj)
        {
            // Not stopped.
            if (obj.ConnectionState != LocalConnectionState.Stopped)
                return;

            // Reset toServer data.
            foreach (PacketBundle pb in _toServerBundles)
                pb.Reset(resetSendLast: true);
        }

        /// <summary>
        /// Called when the local connection state changes for the server.
        /// </summary>
        private void ServerManager_OnServerConnectionState(ServerConnectionStateArgs obj)
        {
            // Not stopped.
            if (obj.ConnectionState != LocalConnectionState.Stopped)
                return;

            // If no server is started just clear all dirtyToClients.
            if (!_networkManager.ServerManager.IsAnyServerStarted())
            {
                _dirtyToClients.Clear();
                return;
            }

            // Only one server is stopped, remove connections for that server.
            int index = obj.TransportIndex;

            List<NetworkConnection> clientsForIndex = CollectionCaches<NetworkConnection>.RetrieveList();
            foreach (NetworkConnection conn in _dirtyToClients)
            {
                if (conn.TransportIndex == index)
                    clientsForIndex.Add(conn);
            }

            foreach (NetworkConnection conn in clientsForIndex)
                _dirtyToClients.Remove(conn);

            CollectionCaches<NetworkConnection>.Store(clientsForIndex);
        }

        /// <summary>
        /// Sets a connection from server to client dirty.
        /// </summary>
        /// <param name = "conn"></param>
        internal void ServerDirty(NetworkConnection conn)
        {
            _dirtyToClients.Add(conn);
        }

        /// <summary>
        /// Initializes ToServerBundles for use.
        /// </summary>
        private void InitializeToServerBundles()
        {
            /* For ease of use FishNet will always have
             * only two channels, reliable and unreliable.
             * Even if the transport only supports reliable
             * also setup for unreliable. */
            for (byte i = 0; i < CHANNEL_COUNT; i++)
            {
                int mtu = GetLowestMTU(i);
                _toServerBundles.Add(new(_networkManager, mtu));
            }
        }

        #region GetMTU.
        /// <summary>
        /// Returns MTU excluding reserve amount.
        /// </summary>
        private int GetMTUWithReserve(int mtu)
        {
            int value = mtu - MINIMUM_MTU_RESERVE - _customMtuReserve;
            /* If MTU is extremely low then warn user.
             * The number choosen has no significant value. */
            if (value <= 100)
            {
                string msg = $"Available MTU of {mtu} is significantly low; an invalid MTU will be returned. Check transport settings, or reduce MTU reserve if you set one using {nameof(SetMTUReserve)}";
                _networkManager.LogWarning(msg);

                return INVALID_MTU;
            }

            return value;
        }

        /// <summary>
        /// Sets a custom value to reserve for the internal buffers.
        /// This value is also deducted from transport MTU when using GetMTU methods.
        /// </summary>
        /// <param name = "value">Value to use.</param>
        public void SetMTUReserve(int value)
        {
            if ((_networkManager != null && _networkManager.IsClientStarted) || _networkManager.IsServerStarted)
            {
                _networkManager.LogError($"A custom MTU reserve cannot be set after the server or client have been started or connected.");
                return;
            }

            if (value < MINIMUM_MTU_RESERVE)
            {
                _networkManager.Log($"MTU reserve {value} is below minimum value of {MINIMUM_MTU_RESERVE}. Value has been updated to {MINIMUM_MTU_RESERVE}.");
                value = MINIMUM_MTU_RESERVE;
            }

            _customMtuReserve = value;
            InitializeToServerBundles();
        }

        /// <summary>
        /// Returns the current MTU reserve.
        /// </summary>
        /// <returns></returns>
        public int GetMTUReserve() => _customMtuReserve;

        /// <summary>
        /// Returns the lowest MTU of all channels. When using multipass this will evaluate all transports within Multipass.
        /// </summary>
        /// <param name = "channel"></param>
        /// <returns></returns>
        public int GetLowestMTU()
        {
            SetLowestMTUs();
            return GetMTUWithReserve(_lowestMtu);
        }

        /// <summary>
        /// Returns the lowest MTU for a channel. When using multipass this will evaluate all transports within Multipass.
        /// </summary>
        /// <param name = "channel"></param>
        /// <returns></returns>
        public int GetLowestMTU(byte channel)
        {
            SetLowestMTUs();
            
            return GetMTUWithReserve(_lowestMtus[channel]);
        }

        /// <summary>
        /// Gets MTU on the current transport for channel.
        /// </summary>
        /// <param name = "channel">Channel to get MTU of.</param>
        /// <returns></returns>
        public int GetMTU(byte channel)
        {
            SetLowestMTUs();

            int mtu = Transport.GetMTU(channel);
            if (mtu == INVALID_MTU)
                return mtu;

            return GetMTUWithReserve(mtu);
        }

        /// <summary>
        /// Gets MTU on the transportIndex for channel. This requires use of Multipass.
        /// </summary>
        /// <param name = "transportIndex">Index of the transport to get the MTU on.</param>
        /// <param name = "channel">Channel to get MTU of.</param>
        /// <returns></returns>
        public int GetMTU(int transportIndex, byte channel)
        {
            if (Transport is Multipass mp)
            {
                int mtu = mp.GetMTU(channel, transportIndex);
                if (mtu == INVALID_MTU)
                    return INVALID_MTU;

                return GetMTUWithReserve(mtu);
            }
            // Using first/only transport.
            if (transportIndex == 0)
                return GetMTU(channel);

            // Unhandled.
            _networkManager.LogWarning($"MTU cannot be returned with transportIndex because {typeof(Multipass).Name} is not in use.");
            return -1;
        }

        /// <summary>
        /// Returns Channel.Reliable if data length is over MTU for the provided channel.
        /// </summary>
        public Channel GetReliableChannelIfOverMTU(int dataLength, Channel currentChannel) => dataLength > GetMTU((byte)currentChannel) ? Channel.Reliable : currentChannel;

        /// <summary>
        /// Gets MTU on the transport type for channel. This requires use of Multipass.
        /// </summary>
        /// <typeparam name = "T">Tyep of transport to use.</typeparam>
        /// <param name = "channel">Channel to get MTU of.</param>
        /// <returns></returns>
        public int GetMTU<T>(byte channel) where T : Transport
        {
            Transport transport = GetTransport<T>();
            if (transport != null)
            {
                int mtu = transport.GetMTU(channel);
                if (mtu == INVALID_MTU)
                    return mtu;

                return GetMTUWithReserve(mtu);
            }

            // Fall through.
            return INVALID_MTU;
        }
        #endregion

        /// <summary>
        /// Passes received to the intermediate layer.
        /// </summary>
        internal ArraySegment<byte> ProcessIntermediateIncoming(ArraySegment<byte> src, bool fromServer)
        {
            return _intermediateLayer.HandleIncoming(src, fromServer);
        }

        /// <summary>
        /// Passes sent to the intermediate layer.
        /// </summary>
        private ArraySegment<byte> ProcessIntermediateOutgoing(ArraySegment<byte> src, bool toServer)
        {
            return _intermediateLayer.HandleOutgoing(src, toServer);
        }

        /// <summary>
        /// Sends data to a client.
        /// </summary>
        /// <param name = "channelId">Channel to send on.</param>
        /// <param name = "segment">Data to send.</param>
        /// <param name = "connection">Connection to send to. Use null for all clients.</param>
        /// <param name = "splitLargeMessages">True to split large packets which exceed MTU and send them in order on the reliable channel.</param>
        internal void SendToClient(byte channelId, ArraySegment<byte> segment, NetworkConnection connection, DataOrderType orderType = DataOrderType.Default)
        {
            channelId = GetFallbackChannelIdAsNeeded(channelId);

            if (SendSplitMessage(connection, channelId, segment, orderType) == SPLIT_NOT_REQUIRED_VALUE)
                connection.SendToClient(channelId, segment, forceNewBuffer: false, orderType);
        }

        /// <summary>
        /// Sends data to observers.
        /// </summary>
        internal void SendToClients(byte channelId, ArraySegment<byte> segment, HashSet<NetworkConnection> observers, HashSet<NetworkConnection> excludedConnections = null, DataOrderType orderType = DataOrderType.Default)
        {
            if (excludedConnections == null || excludedConnections.Count == 0)
            {
                foreach (NetworkConnection conn in observers)
                    SendToClient(channelId, segment, conn, orderType);
            }
            else
            {
                foreach (NetworkConnection conn in observers)
                {
                    if (excludedConnections.Contains(conn))
                        continue;

                    SendToClient(channelId, segment, conn, orderType);
                }
            }
        }

        /// <summary>
        /// Sends data to all clients.
        /// </summary>
        /// <param name = "channelId">Channel to send on.</param>
        /// <param name = "segment">Data to send.</param>
        /// <param name = "splitLargeMessages">True to split large packets which exceed MTU and send them in order on the reliable channel.</param>
        internal void SendToClients(byte channelId, ArraySegment<byte> segment)
        {
            /* Rather than buffer the message once and send to every client
             * it must be queued into every client. This ensures clients
             * receive the message in order of other packets being
             * delivered to them. */
            foreach (NetworkConnection conn in _networkManager.ServerManager.Clients.Values)
                SendToClient(channelId, segment, conn);
        }

        /// <summary>
        /// Sends data to the server.
        /// </summary>
        /// <param name = "channelId">Channel to send on.</param>
        /// <param name = "segment">Data to send.</param>
        /// <param name = "splitLargeMessages">True to split large packets which exceed MTU and send them in order on the reliable channel.</param>
        internal void SendToServer(byte channelId, ArraySegment<byte> segment, DataOrderType orderType = DataOrderType.Default)
        {
            channelId = GetFallbackChannelIdAsNeeded(channelId);

            if (SendSplitMessage(conn: null, channelId, segment, orderType) == SPLIT_NOT_REQUIRED_VALUE)
                _toServerBundles[channelId].Write(segment, forceNewBuffer: false, orderType);
        }

        /// <summary>
        /// Gets the channelId to use, returning a fallback Id if the provided channelId is not supported.
        /// </summary>
        private byte GetFallbackChannelIdAsNeeded(byte channelId) => channelId > _toServerBundles.Count ? (byte)Channel.Reliable : channelId;

        /// <summary>
        /// Splits data going to which is too large to fit within the transport MTU.
        /// </summary>
        /// <param name = "conn">Connection to send to. If null data will be sent to the server.</param>
        /// <returns>True if data was sent split.</returns>
        private int SendSplitMessage(NetworkConnection conn, byte channelId, ArraySegment<byte> segment, DataOrderType orderType)
        {
            int lowestMTU = GetLowestMTU(channelId);
            int segmentCount = segment.Count;

            //Splitting is not required.
            if (segmentCount <= lowestMTU)
                //0 indicates no split required.
                return SPLIT_NOT_REQUIRED_VALUE;

            int maximumSegmentLength = _maximumSplitPacketSegmentLength;
            int messageCount = (int)Math.Ceiling((double)segmentCount / maximumSegmentLength);

            /* If going to the server and value exceeds the
             * maximum segment size then the data cannot be sent. */
            if (conn == null && messageCount * maximumSegmentLength > _maximumClientPacketSize)
            {
                _networkManager.LogError($"A packet of length {segmentCount} cannot be sent because it exceeds the maximum packet size allowed by a client of {_maximumClientPacketSize}.");
                return SPLIT_ERROR_VALUE;
            }

            //Writer used to write the header and segment of each split message.
            PooledWriter splitWriter = WriterPool.Retrieve();

            //Channel is forced to reliable for split messages.
            channelId = SPLIT_PACKET_CHANNELID;

            for (int i = 0; i < messageCount; i++)
            {
                splitWriter.WritePacketIdUnpacked(PacketId.Split);
                splitWriter.WriteInt32(messageCount);

                int startPosition = i * maximumSegmentLength;

                int chunkSize = Mathf.Min(segment.Count - startPosition, maximumSegmentLength);
                ArraySegment<byte> splitSegment = new(segment.Array, segment.Offset + startPosition, chunkSize);
                splitWriter.WriteArraySegment(splitSegment);
                
                // If connection is specified then it's going to a client.
                if (conn != null)
                    conn.SendToClient(channelId, splitWriter.GetArraySegment());
                // Otherwise it's going to the server.
                else
                    _toServerBundles[channelId].Write(splitWriter.GetArraySegment(), forceNewBuffer: false, orderType);

                splitWriter.Clear();
            }

            WriterPool.Store(splitWriter);

            return SPLIT_SENT_VALUE;
        }

        /// <summary>
        /// Processes data received by the socket.
        /// </summary>
        /// <param name = "asServer">True to read data from clients, false to read data from the server.
        internal void IterateIncoming(bool asServer)
        {
            OnIterateIncomingStart?.Invoke(asServer);
            Transport.IterateIncoming(asServer);
            OnIterateIncomingEnd?.Invoke(asServer);
        }

        /// <summary>
        /// Processes data to be sent by the socket.
        /// </summary>
        /// <param name = "asServer">True to send data from the local server to clients, false to send from the local client to server.
        internal void IterateOutgoing(bool asServer)
        {
            if (asServer && _networkManager.ServerManager.AreAllServersStopped())
                return;

            OnIterateOutgoingStart?.Invoke();
            int channelCount = CHANNEL_COUNT;
            ulong sentBytes = 0;
            #if DEVELOPMENT
            bool latencySimulatorEnabled = LatencySimulator.CanSimulate;
            #endif
            if (asServer)
                SendAsServer();
            else
                SendAsClient();

            // Sends data as server.
            void SendAsServer()
            {
                TimeManager tm = _networkManager.TimeManager;
                uint localTick = tm.LocalTick;
                // Write any dirty syncTypes.
                _networkManager.ServerManager.Objects.WriteDirtySyncTypes();

                int dirtyCount = _dirtyToClients.Count;

                // Run through all dirty connections to send data to.
                for (int z = 0; z < dirtyCount; z++)
                {
                    NetworkConnection conn = _dirtyToClients[z];
                    if (conn == null || !conn.IsValid)
                        continue;

                    // Get packets for every channel.
                    for (byte channel = 0; channel < channelCount; channel++)
                    {
                        if (conn.GetPacketBundle(channel, out PacketBundle pb))
                        {
                            ProcessPacketBundle(pb);
                            ProcessPacketBundle(pb.GetSendLastBundle(), true);

                            void ProcessPacketBundle(PacketBundle ppb, bool isLast = false)
                            {
                                for (int i = 0; i < ppb.WrittenBuffers; i++)
                                {
                                    // Length should always be more than 0 but check to be safe.
                                    if (ppb.GetBuffer(i, out ByteBuffer bb))
                                    {
                                        ArraySegment<byte> segment = new(bb.Data, 0, bb.Length);
                                        if (HasIntermediateLayer)
                                            segment = ProcessIntermediateOutgoing(segment, false);
                                        #if DEVELOPMENT
                                        if (latencySimulatorEnabled)
                                            _latencySimulator.AddOutgoing(channel, segment, false, conn.ClientId);
                                        else
                                            #endif
                                            Transport.SendToClient(channel, segment, conn.ClientId);
                                        sentBytes += (ulong)segment.Count;
                                    }
                                }

                                ppb.Reset(false);
                            }
                        }
                    }

                    /* When marked as disconnecting data will still be sent
                     * this iteration but the connection will be marked as invalid.
                     * This will prevent future data from going out/coming in.
                     * Also the connection will be added to a disconnecting collection
                     * so it will it disconnected briefly later to allow data from
                     * this tick to send. */
                    if (conn.Disconnecting)
                    {
                        uint requiredTicks = tm.TimeToTicks(0.1d, TickRounding.RoundUp);
                        /* Require 100ms or 2 ticks to pass
                         * before disconnecting to allow for the
                         * higher chance of success that remaining
                         * data is sent. */
                        requiredTicks = Math.Max(requiredTicks, 2);
                        _disconnectingClients.Add(new(requiredTicks + localTick, conn));
                    }

                    conn.ResetServerDirty();
                }

                // Iterate disconnects.
                for (int i = 0; i < _disconnectingClients.Count; i++)
                {
                    DisconnectingClient dc = _disconnectingClients[i];
                    if (localTick >= dc.Tick)
                    {
                        _networkManager.TransportManager.Transport.StopConnection(dc.Connection.ClientId, true);
                        _disconnectingClients.RemoveAt(i);
                        i--;
                    }
                }

                if (_networkTrafficStatistics != null)
                    _networkTrafficStatistics.AddOutboundSocketData(sentBytes, asServer: true);

                if (dirtyCount == _dirtyToClients.Count)
                    _dirtyToClients.Clear();
                else if (dirtyCount > 0)
                    _dirtyToClients.RemoveRange(0, dirtyCount);
            }

            // Sends data as client.
            void SendAsClient()
            {
                for (byte channel = 0; channel < channelCount; channel++)
                {
                    if (PacketBundle.GetPacketBundle(channel, _toServerBundles, out PacketBundle pb))
                    {
                        ProcessPacketBundle(pb);
                        ProcessPacketBundle(pb.GetSendLastBundle());

                        void ProcessPacketBundle(PacketBundle ppb)
                        {
                            for (int i = 0; i < ppb.WrittenBuffers; i++)
                            {
                                if (ppb.GetBuffer(i, out ByteBuffer bb))
                                {
                                    ArraySegment<byte> segment = new(bb.Data, 0, bb.Length);
                                    if (HasIntermediateLayer)
                                        segment = ProcessIntermediateOutgoing(segment, true);
                                    #if DEVELOPMENT
                                    if (latencySimulatorEnabled)
                                        _latencySimulator.AddOutgoing(channel, segment);
                                    else
                                        #endif
                                        Transport.SendToServer(channel, segment);
                                    sentBytes += (ulong)segment.Count;
                                }
                            }

                            ppb.Reset(false);
                        }
                    }
                }

                if (_networkTrafficStatistics != null)
                    _networkTrafficStatistics.AddOutboundSocketData(sentBytes, asServer: false);
            }

            #if DEVELOPMENT
            if (latencySimulatorEnabled)
                _latencySimulator.IterateOutgoing(asServer);
            #endif

            Transport.IterateOutgoing(asServer);
            OnIterateOutgoingEnd?.Invoke();
        }

        #region Editor.
        #if UNITY_EDITOR
        private void OnValidate()
        {
            if (Transport == null)
                Transport = GetComponent<Transport>();

            /* Update enabled state to force a reset if needed.
             * This may be required if the user checked the enabled
             * tick box at runtime. If enabled value didn't change
             * then the Get will be the same as the Set and nothing
             * will happen. */
            _latencySimulator.SetEnabled(_latencySimulator.GetEnabled());
        }
        #endif
        #endregion
    }
}