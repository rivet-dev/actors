#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Connection;
using FishNet.Managing.Debugging;
using FishNet.Managing.Logging;
using FishNet.Managing.Server;
using FishNet.Managing.Timing;
using FishNet.Managing.Transporting;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Transporting.Multipass;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using FishNet.Managing.Statistic;
using Unity.Profiling;
using UnityEngine;

namespace FishNet.Managing.Client
{
    /// <summary>
    /// A container for local client data and actions.
    /// </summary>
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Manager/ClientManager")]
    public sealed partial class ClientManager : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// This is set true if the server has notified the client it is using a development build.
        /// Value is set before authentication.
        /// </summary>
        public bool IsServerDevelopment { get; private set; }
        /// <summary>
        /// Called after local client has authenticated.
        /// </summary>
        public event Action OnAuthenticated;
        /// <summary>
        /// Called when the local client connection to the server has timed out.
        /// This is called immediately before disconnecting.
        /// </summary>
        public event Action OnClientTimeOut;
        /// <summary>
        /// Called after the local client connection state changes.
        /// </summary>
        public event Action<ClientConnectionStateArgs> OnClientConnectionState;
        /// <summary>
        /// Called when a client other than self connects.
        /// This is only available when using ServerManager.ShareIds.
        /// </summary>
        public event Action<RemoteConnectionStateArgs> OnRemoteConnectionState;
        /// <summary>
        /// Called when the server sends all currently connected clients.
        /// This is only available when using ServerManager.ShareIds.
        /// </summary>
        public event Action<ConnectedClientsArgs> OnConnectedClients;
        /// <summary>
        /// True if the client connection is connected to the server.
        /// </summary>
        public bool Started { get; private set; }
        /// <summary>
        /// NetworkConnection the local client is using to send data to the server.
        /// </summary>
        public NetworkConnection Connection = NetworkManager.EmptyConnection;
        /// <summary>
        /// Handling and information for objects known to the local client.
        /// </summary>
        public ClientObjects Objects { get; private set; }
        /// <summary>
        /// All currently connected clients. This field only contains data while ServerManager.ShareIds is enabled.
        /// </summary>
        public Dictionary<int, NetworkConnection> Clients = new();
        /// <summary>
        /// NetworkManager for client.
        /// </summary>
        [HideInInspector]
        public NetworkManager NetworkManager { get; private set; }
        #endregion

        #region Serialized.
        /// <summary>
        /// What platforms to enable remote server timeout.
        /// </summary>
        [Tooltip("What platforms to enable remote server timeout.")]
        [SerializeField]
        private RemoteTimeoutType _remoteServerTimeout = RemoteTimeoutType.Development;
        /// <summary>
        /// How long in seconds server must go without sending any packets before the local client disconnects. This is independent of any transport settings.
        /// </summary>
        [Tooltip("How long in seconds server must go without sending any packets before the local client disconnects. This is independent of any transport settings.")]
        [Range(1, ServerManager.MAXIMUM_REMOTE_CLIENT_TIMEOUT_DURATION)]
        [SerializeField]
        private ushort _remoteServerTimeoutDuration = 60;

        /// <summary>
        /// Sets timeout settings. Can be used at runtime.
        /// </summary>
        /// <returns></returns>
        public void SetRemoteServerTimeout(RemoteTimeoutType timeoutType, ushort duration)
        {
            _remoteServerTimeout = timeoutType;
            duration = (ushort)Mathf.Clamp(duration, 1, ServerManager.MAXIMUM_REMOTE_CLIENT_TIMEOUT_DURATION);
            _remoteServerTimeoutDuration = duration;
        }

        // todo add remote server timeout (see ServerManager.RemoteClientTimeout).
        /// <summary>
        /// True to automatically set the frame rate when the client connects.
        /// </summary>
        [Tooltip("True to automatically set the frame rate when the client connects.")]
        [SerializeField]
        private bool _changeFrameRate = true;
        /// <summary>
        /// Maximum frame rate the client may run at. When as host this value runs at whichever is higher between client and server.
        /// </summary>
        internal ushort FrameRate => _changeFrameRate ? _frameRate : (ushort)0;
        [Tooltip("Maximum frame rate the client may run at. When as host this value runs at whichever is higher between client and server.")]
        [Range(1, NetworkManager.MAXIMUM_FRAMERATE)]
        [SerializeField]
        private ushort _frameRate = NetworkManager.MAXIMUM_FRAMERATE;

        /// <summary>
        /// Sets the maximum frame rate the client may run at. Calling this method will enable ChangeFrameRate.
        /// </summary>
        /// <param name = "value">New value.</param>
        public void SetFrameRate(ushort value)
        {
            _frameRate = (ushort)Mathf.Clamp(value, 0, NetworkManager.MAXIMUM_FRAMERATE);
            _changeFrameRate = true;
            if (NetworkManager != null)
                NetworkManager.UpdateFramerate();
        }
        #endregion

        #region Private.
        /// <summary>
        /// Last unscaled time client got a packet.
        /// </summary>
        private float _lastPacketTime;
        /// <summary>
        /// Used to read splits.
        /// </summary>
        private SplitReader _splitReader;

        /// <summary>
        /// Gets the current SplitReader.
        /// </summary>
        /// <returns></returns>
        private bool TryGetSplitReader(int expectedMessages, out SplitReader splitReader)
        {
            if (_splitReader == null)
            {
                if (NetworkManager is null)
                {
                    NetworkManagerExtensions.LogError($"SplitReader cannot be returned because the NetworkManager reference is null.");
                    splitReader = null;

                    return false;
                }

                _splitReader = ResettableObjectCaches<SplitReader>.Retrieve();
                _splitReader.Initialize(NetworkManager, NetworkManager.TransportManager.MaximumClientPacketSize, isSenderClient: false, expectedMessages);
            }

            splitReader = _splitReader;
            return true;
        }

        /// <summary>
        /// </summary>
        [NonSerialized]
        private NetworkTrafficStatistics _networkTrafficStatistics;
        #endregion

        #region Private Profiler Markers
        private static readonly ProfilerMarker _pm_OnPostTick = new("ClientManager.TimeManager_OnPostTick()");
        private static readonly ProfilerMarker _pm_Transport_OnClientConnectionState = new("ClientManager.Transport_OnClientConnectionState(ClientConnectionStateArgs)");
        private static readonly ProfilerMarker _pm_Transport_OnClientReceivedData = new("ClientManager.Transport_OnClientReceivedData(ClientReceivedDataArgs)");
        private static readonly ProfilerMarker _pm_TransportManager_OnIterateIncomingEnd = new("ClientManager.TransportManager_OnIterateIncomingEnd(bool)");
        private static readonly ProfilerMarker _pm_ParseReceived = new("ClientManager.ParseReceived(ClientReceivedDataArgs)");
        private static readonly ProfilerMarker _pm_ParseReader = new("ClientManager.ParseReader(PooledReader, Channel, bool)");
        private static readonly ProfilerMarker _pm_ParseReader_ReadPacketId = new("ClientManager.ParseReader.ReadPacketId()");
        private static readonly ProfilerMarker _pm_ParseReader_HandlePacket = new("ClientManager.ParseReader.HandlePacket()");
        private static readonly ProfilerMarker _pm_ParseReader_StateUpdate = new("ClientManager.ParseReader.StateUpdate()");
        private static readonly ProfilerMarker _pm_ParseReader_Broadcast = new("ClientManager.ParseReader.Broadcast()");
        private static readonly ProfilerMarker _pm_ParseReader_PingPong = new("ClientManager.ParseReader.PingPong()");
        private static readonly ProfilerMarker _pm_ParseReader_TimingUpdate = new("ClientManager.ParseReader.TimingUpdate()");
        private static readonly ProfilerMarker _pm_ParseReader_Authenticated = new("ClientManager.ParseReader.Authenticated()");
        private static readonly ProfilerMarker _pm_ParseReader_Disconnect = new("ClientManager.ParseReader.Disconnect()");
        private static readonly ProfilerMarker _pm_ParseReader_Version = new("ClientManager.ParseReader.Version()");
        #endregion

        private void OnDestroy()
        {
            Objects?.SubscribeToSceneLoaded(false);
        }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        /// <param name = "manager"></param>
        internal void InitializeOnce_Internal(NetworkManager manager)
        {
            NetworkManager = manager;

            manager.StatisticsManager.TryGetNetworkTrafficStatistics(out _networkTrafficStatistics);

            Objects = new(manager);
            Objects.SubscribeToSceneLoaded(true);

            /* Unsubscribe before subscribing.
             * Shouldn't be an issue but better safe than sorry. */
            SubscribeToEvents(false);
            SubscribeToEvents(true);
            // Listen for client connections from server.
            RegisterBroadcast<ClientConnectionChangeBroadcast>(OnClientConnectionBroadcast);
            RegisterBroadcast<ConnectedClientsBroadcast>(OnConnectedClientsBroadcast);
        }

        /// <summary>
        /// Called when the server sends a connection state change for any client.
        /// </summary>
        /// <param name = "args"></param>
        private void OnClientConnectionBroadcast(ClientConnectionChangeBroadcast args, Channel channel)
        {
            // If connecting invoke after added to clients, otherwise invoke before removed.
            RemoteConnectionStateArgs rcs = new(args.Connected ? RemoteConnectionState.Started : RemoteConnectionState.Stopped, args.Id, -1);

            if (args.Connected)
            {
                Clients[args.Id] = new(NetworkManager, args.Id, -1, false);
                OnRemoteConnectionState?.Invoke(rcs);
            }
            else
            {
                OnRemoteConnectionState?.Invoke(rcs);
                if (Clients.TryGetValue(args.Id, out NetworkConnection c))
                {
                    c.ResetState();
                    Clients.Remove(args.Id);
                }
            }
        }

        /// <summary>
        /// Called when the server sends all currently connected clients.
        /// </summary>
        /// <param name = "args"></param>
        private void OnConnectedClientsBroadcast(ConnectedClientsBroadcast args, Channel channel)
        {
            NetworkManager.ClearClientsCollection(Clients);

            List<int> collection = args.Values;
            // No connected clients except self.
            if (collection == null)
            {
                collection = new();
            }
            // Other clients.
            else
            {
                int count = collection.Count;
                for (int i = 0; i < count; i++)
                {
                    int id = collection[i];
                    Clients[id] = new(NetworkManager, id, -1, false);
                }
            }

            OnConnectedClients?.Invoke(new(collection));
        }

        /// <summary>
        /// Changes subscription status to transport.
        /// </summary>
        /// <param name = "subscribe"></param>
        private void SubscribeToEvents(bool subscribe)
        {
            if (NetworkManager == null || NetworkManager.TransportManager == null || NetworkManager.TransportManager.Transport == null)
                return;

            if (subscribe)
            {
                NetworkManager.TransportManager.OnIterateIncomingEnd += TransportManager_OnIterateIncomingEnd;
                NetworkManager.TransportManager.Transport.OnClientReceivedData += Transport_OnClientReceivedData;
                NetworkManager.TransportManager.Transport.OnClientConnectionState += Transport_OnClientConnectionState;
                NetworkManager.TimeManager.OnPostTick += TimeManager_OnPostTick;
            }
            else
            {
                NetworkManager.TransportManager.OnIterateIncomingEnd -= TransportManager_OnIterateIncomingEnd;
                NetworkManager.TransportManager.Transport.OnClientReceivedData -= Transport_OnClientReceivedData;
                NetworkManager.TransportManager.Transport.OnClientConnectionState -= Transport_OnClientConnectionState;
                NetworkManager.TimeManager.OnPostTick -= TimeManager_OnPostTick;
            }
        }

        /// <summary>
        /// Gets the transport index being used for the local client.
        /// If only one transport is used this will return 0. If Multipass is being used this will return the client's transport in multipass.
        /// </summary>
        /// <returns></returns>
        public int GetTransportIndex()
        {
            if (NetworkManager.TransportManager.Transport is Multipass mp)
                return mp.ClientTransport.Index;
            else
                return 0;
        }

        /// <summary>
        /// Stops the local client connection.
        /// </summary>
        public bool StopConnection()
        {
            return NetworkManager.TransportManager.Transport.StopConnection(false);
        }

        /// <summary>
        /// Starts the local client connection.
        /// </summary>
        public bool StartConnection()
        {
            return NetworkManager.TransportManager.Transport.StartConnection(false);
        }

        /// <summary>
        /// Sets the transport address and starts the local client connection.
        /// </summary>
        public bool StartConnection(string address)
        {
            NetworkManager.TransportManager.Transport.SetClientAddress(address);
            return StartConnection();
        }

        /// <summary>
        /// Sets the transport address and port, and starts the local client connection.
        /// </summary>
        public bool StartConnection(string address, ushort port)
        {
            NetworkManager.TransportManager.Transport.SetClientAddress(address);
            NetworkManager.TransportManager.Transport.SetPort(port);
            return StartConnection();
        }

        /// <summary>
        /// Called when a connection state changes for the local client.
        /// </summary>
        /// <param name = "args"></param>
        private void Transport_OnClientConnectionState(ClientConnectionStateArgs args)
        {
            using (_pm_Transport_OnClientConnectionState.Auto())
            {
                LocalConnectionState state = args.ConnectionState;
                Started = state == LocalConnectionState.Started;
                Objects.OnClientConnectionState(args);

                // Clear connection after so objects can update using current Connection value.
                if (!Started)
                {
                    Connection = NetworkManager.EmptyConnection;
                    NetworkManager.ClearClientsCollection(Clients);
                }
                else
                {
                    _lastPacketTime = Time.unscaledTime;
                    // Send version.
                    PooledWriter writer = WriterPool.Retrieve();
                    writer.WritePacketIdUnpacked(PacketId.Version);
                    writer.WriteString(NetworkManager.FISHNET_VERSION);
                    NetworkManager.TransportManager.SendToServer((byte)Channel.Reliable, writer.GetArraySegment());
                    WriterPool.Store(writer);
                }

                if (NetworkManager.CanLog(LoggingType.Common))
                {
                    Transport t = NetworkManager.TransportManager.GetTransport(args.TransportIndex);
                    string tName = t == null ? "Unknown" : t.GetType().Name;
                    string socketInformation = string.Empty;
                    if (state == LocalConnectionState.Starting)
                        socketInformation = $" Server IP is {t.GetClientAddress()}, port is {t.GetPort()}.";
                    NetworkManager.Log($"Local client is {state.ToString().ToLower()} for {tName}.{socketInformation}");
                }

                NetworkManager.UpdateFramerate();
                OnClientConnectionState?.Invoke(args);
            }
        }

        /// <summary>
        /// Called when a socket receives data.
        /// </summary>
        private void Transport_OnClientReceivedData(ClientReceivedDataArgs args)
        {
            using (_pm_Transport_OnClientReceivedData.Auto())
            {
                ParseReceived(args);
            }
        }

        /// <summary>
        /// Called after IterateIncoming has completed.
        /// </summary>
        private void TransportManager_OnIterateIncomingEnd(bool server)
        {
            using (_pm_TransportManager_OnIterateIncomingEnd.Auto())
            {
                /* Should the last packet received be a spawn or despawn
                 * then the cache won't yet be iterated because it only
                 * iterates when a packet is anything but those two. Because
                 * of such if any object caches did come in they must be iterated
                 * at the end of the incoming cycle. This isn't as clean as I'd
                 * like but it does ensure there will be no missing network object
                 * references on spawned objects. */
                if (Started && !server)
                    Objects.IterateObjectCache();
            }
        }

        /// <summary>
        /// Parses received data.
        /// </summary>
        private void ParseReceived(ClientReceivedDataArgs args)
        {
            using (_pm_ParseReceived.Auto())
            {
                #if DEVELOPMENT && !UNITY_SERVER
                if (_networkTrafficStatistics != null)
                    _networkTrafficStatistics.PacketBundleReceived(asServer: false);
                #endif

                _lastPacketTime = Time.unscaledTime;

                ArraySegment<byte> segment;
                if (NetworkManager.TransportManager.HasIntermediateLayer)
                    segment = NetworkManager.TransportManager.ProcessIntermediateIncoming(args.Data, true);
                else
                    segment = args.Data;

                if (_networkTrafficStatistics != null)
                    _networkTrafficStatistics.AddInboundSocketData((ulong)segment.Count, asServer: false);

                if (segment.Count <= TransportManager.UNPACKED_TICK_LENGTH)
                    return;

                PooledReader reader = ReaderPool.Retrieve(segment, NetworkManager, Reader.DataSource.Server);
                TimeManager tm = NetworkManager.TimeManager;
                tm.LastPacketTick.Update(reader.ReadTickUnpacked(), EstimatedTick.OldTickOption.Discard, false);
                ParseReader(reader, args.Channel);
                ReaderPool.Store(reader);
            }
        }

        internal void ParseReader(PooledReader reader, Channel channel, bool print = false)
        {
            using (_pm_ParseReader.Auto())
            {
                PacketId packetId = PacketId.Unset;
                #if !DEVELOPMENT
                try
                {
                #endif
                Reader.DataSource dataSource = Reader.DataSource.Server;
                /* This is a special condition where a message may arrive split.
                 * When this occurs buffer each packet until all packets are
                 * received. */
                if (reader.PeekPacketId() == PacketId.Split)
                {
                    #if DEVELOPMENT
                    NetworkManager.PacketIdHistory.ReceivedPacket(PacketId.Split, packetFromServer: true);
                    #endif
                    // Skip packetId.
                    reader.ReadPacketId();

                    int expectedMessages = reader.ReadInt32();

                    if (!TryGetSplitReader(expectedMessages, out SplitReader splitReader))
                    {
                        NetworkManager.LogError($"Something went wrong when trying to get the [{nameof(splitReader)}] for a server message.");
                        return;
                    }

                    if (!splitReader.Write(reader))
                    {
                        NetworkManager.LogError($"Something went wrong when writing a split message from the server.");
                        return;
                    }

                    /* If fullMessage returns 0 count then the split
                     * has not written fully yet. Otherwise, if there is
                     * data within then reinitialize reader with the
                     * full message. */
                    if (!splitReader.TryGetFullMessage(out ArraySegment<byte> fullMessage))
                        return;

                    reader.Initialize(fullMessage, NetworkManager, dataSource);

                    //Once here the split reader can be stored.
                    ResettableObjectCaches<SplitReader>.StoreAndDefault(ref _splitReader);
                }

                while (reader.Remaining > 0)
                {
                    using (_pm_ParseReader_ReadPacketId.Auto())
                    {
                        packetId = reader.ReadPacketId();
                        #if DEVELOPMENT
                        NetworkManager.PacketIdHistory.ReceivedPacket(packetId, packetFromServer: true);
                        #endif
                    }
                    bool spawnOrDespawn = packetId == PacketId.ObjectSpawn || packetId == PacketId.ObjectDespawn;
                    /* Length of data. Only available if using unreliable. Unreliable packets
                     * can arrive out of order which means object orientated messages such as RPCs may
                     * arrive after the object for which they target has already been destroyed. When this happens
                     * on lesser solutions they just dump the entire packet. However, since FishNet batches data.
                     * it's very likely a packet will contain more than one packetId. With this mind, length is
                     * sent as well so if any reason the data does have to be dumped it will only be dumped for
                     * that single packetId  but not the rest. Broadcasts don't need length either even if unreliable
                     * because they are not object bound. */

                    // Is spawn or despawn; cache packet.
                    if (spawnOrDespawn)
                    {
                        if (packetId == PacketId.ObjectSpawn)
                            Objects.ReadSpawn(reader);
                        else if (packetId == PacketId.ObjectDespawn)
                            Objects.CacheDespawn(reader);
                    }
                    // Not spawn or despawn.
                    else
                    {
                        /* Iterate object cache should any of the
                         * incoming packets rely on it. Objects
                         * in cache will always be received before any messages
                         * that use them. */
                        Objects.IterateObjectCache();
                        using (_pm_ParseReader_HandlePacket.Auto())
                        {
                            // Then process packet normally.
                            if ((ushort)packetId >= NetworkManager.StartingRpcLinkIndex)
                            {
                                Objects.ParseRpcLink(reader, (ushort)packetId, channel);
                            }
                            else if (packetId == PacketId.StateUpdate)
                            {
                                using (_pm_ParseReader_StateUpdate.Auto())
                                {
                                    NetworkManager.PredictionManager.ParseStateUpdate(reader, channel);
                                }
                            }
                            else if (packetId == PacketId.Replicate)
                            {
                                Objects.ParseReplicateRpc(reader, null, channel);
                            }
                            else if (packetId == PacketId.Reconcile)
                            {
                                Objects.ParseReconcileRpc(reader, channel);
                            }
                            else if (packetId == PacketId.ObserversRpc)
                            {
                                Objects.ParseObserversRpc(reader, channel);
                            }
                            else if (packetId == PacketId.TargetRpc)
                            {
                                Objects.ParseTargetRpc(reader, channel);
                            }
                            else if (packetId == PacketId.Broadcast)
                            {
                                using (_pm_ParseReader_Broadcast.Auto())
                                {
                                    ParseBroadcast(reader, channel);
                                }
                            }
                            else if (packetId == PacketId.PingPong)
                            {
                                using (_pm_ParseReader_PingPong.Auto())
                                {
                                    ParsePingPong(reader);
                                }
                            }
                            else if (packetId == PacketId.SyncType)
                            {
                                Objects.ParseSyncType(reader, channel);
                            }
                            else if (packetId == PacketId.PredictedSpawnResult)
                            {
                                Objects.ParsePredictedSpawnResult(reader);
                            }
                            else if (packetId == PacketId.TimingUpdate)
                            {
                                using (_pm_ParseReader_TimingUpdate.Auto())
                                {
                                    NetworkManager.TimeManager.ParseTimingUpdate(reader);
                                }
                            }
                            else if (packetId == PacketId.OwnershipChange)
                            {
                                Objects.ParseOwnershipChange(reader);
                            }
                            else if (packetId == PacketId.Authenticated)
                            {
                                using (_pm_ParseReader_Authenticated.Auto())
                                {
                                    ParseAuthenticated(reader);
                                }
                            }
                            else if (packetId == PacketId.Disconnect)
                            {
                                using (_pm_ParseReader_Disconnect.Auto())
                                {
                                    reader.Clear();
                                    StopConnection();
                                }
                            }
                            else if (packetId == PacketId.Version)
                            {
                                using (_pm_ParseReader_Version.Auto())
                                {
                                    ParseVersion(reader);
                                }
                            }
                            else
                            {
                                NetworkManager.LogError($"Client received an unhandled PacketId of {(ushort)packetId} on channel {channel}. Remaining data has been purged.");
                                #if DEVELOPMENT
                                NetworkManager.LogError(NetworkManager.PacketIdHistory.GetReceivedPacketIds(packetsFromServer: true));
                                #endif
                                return;
                            }
                        }
                    }

                    #if DEVELOPMENT
                    if (print)
                        Debug.Log($"Reader remaining {reader.Remaining}");
                    #endif
                }

                /* Iterate cache when reader is emptied.
                 * This is incase the last packet received
                 * was a spawned, which wouldn't trigger
                 * the above iteration. There's no harm
                 * in doing this check multiple times as there's
                 * an exit early check. */
                Objects.IterateObjectCache();
                #if !DEVELOPMENT
                }
                catch (Exception e)
                {
                    NetworkManager.LogError($"Client encountered an error while parsing data for packetId {packetId}. Message: {e.Message}.");
                }
                #endif
            }
        }

        /// <summary>
        /// Parses a PingPong packet.
        /// </summary>
        /// <param name = "reader"></param>
        private void ParsePingPong(PooledReader reader)
        {
            int readerPositionAfterDebug = reader.Position;

            uint clientTick = reader.ReadTickUnpacked();
            NetworkManager.TimeManager.ModifyPing(clientTick);

            #if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.AddInboundPacketIdData(PacketId.PingPong, string.Empty, reader.Position - readerPositionAfterDebug + TransportManager.PACKETID_LENGTH, gameObject: null, asServer: false);
            #endif
        }

        /// <summary>
        /// Parses a Version packet.
        /// </summary>
        /// <param name = "reader"></param>
        private void ParseVersion(PooledReader reader)
        {
            IsServerDevelopment = reader.ReadBoolean();
        }

        /// <summary>
        /// Parses a received connectionId. This is received before client receives connection state change.
        /// </summary>
        /// <param name = "reader"></param>
        private void ParseAuthenticated(PooledReader reader)
        {
            NetworkManager networkManager = NetworkManager;
            int connectionId = reader.ReadNetworkConnectionId();

            bool isServerStarted = networkManager.IsServerStarted;

            // If only a client then make a new connection.
            if (!isServerStarted)
            {
                Clients.TryGetValueIL2CPP(connectionId, out Connection);
                /* This is bad and should never happen unless the connection is dropping
                 * while receiving authenticated. Would have to be a crazy race condition
                 * but with the network anything is possible. */
                if (Connection == null)
                {
                    NetworkManager.LogWarning($"Client connection could not be found while parsing authenticated status. This usually occurs when the client is receiving a packet immediately before losing connection.");
                    Connection = new(networkManager, connectionId, GetTransportIndex(), false);
                }
            }
            /* If also the server then use the servers connection
             * for the connectionId. This is to resolve host problems
             * where LocalConnection for client differs from the server Connection
             * reference, which results in different field values. */
            else
            {
                if (networkManager.ServerManager.Clients.TryGetValueIL2CPP(connectionId, out NetworkConnection conn))
                {
                    Connection = conn;
                }
                else
                {
                    networkManager.LogError($"Unable to lookup LocalConnection for {connectionId} as host.");
                    Connection = new(networkManager, connectionId, GetTransportIndex(), false);
                }
            }

            // If predicted spawning is enabled also get reserved Ids.
            if (NetworkManager.ServerManager.GetAllowPredictedSpawning())
            {
                int count = (int)reader.ReadSignedPackedWhole();
                Queue<int> q = Connection.PredictedObjectIds;

                for (int i = 0; i < count; i++)
                {
                    /* If host then just read Ids, but do not
                     * enqueue as server side already did so. */
                    int id = reader.ReadNetworkObjectId();
                    if (!isServerStarted)
                        q.Enqueue(id);
                }
            }

            /* Set the TimeManager tick to lastReceivedTick.
             * This still doesn't account for latency but
             * it's the best we can do until the client gets
             * a ping response. */
            if (!networkManager.IsServerStarted)
                networkManager.TimeManager.Tick = networkManager.TimeManager.LastPacketTick.LastRemoteTick;

            // Mark as authenticated.
            Connection.ConnectionAuthenticated();
            OnAuthenticated?.Invoke();
            /* Register scene objects for all scenes
             * after being authenticated. This is done after
             * authentication rather than when the connection
             * is started because if also as server an online
             * scene may already be loaded on server, but not
             * for client. This means the sceneLoaded unity event
             * won't fire, and since client isn't authenticated
             * at the connection start phase objects won't be added. */
            Objects.RegisterAndDespawnSceneObjects();
        }

        /// <summary>
        /// Called when the TimeManager calls OnPostTick.
        /// </summary>
        private void TimeManager_OnPostTick()
        {
            using (_pm_OnPostTick.Auto())
            {
                CheckServerTimeout();
            }
        }

        /// <summary>
        /// Checks to timeout client connections.
        /// </summary>
        private void CheckServerTimeout()
        {
            /* Not connected or host. There should be no way
             * for server to drop and client not know about it as host.
             * This would mean a game crash or force close in which
             * the client would be gone as well anyway. */
            if (!Started || NetworkManager.IsServerStarted)
                return;
            if (_remoteServerTimeout == RemoteTimeoutType.Disabled)
                return;
            #if DEVELOPMENT
            // If development but not set to development return.
            if (_remoteServerTimeout != RemoteTimeoutType.Development)
                return;
            #endif
            // Wait two timing intervals to give packets a chance to come through.
            if (NetworkManager.SceneManager.IsIteratingQueue(2f))
                return;

            /* ServerManager version only checks every so often
             * to perform iterations over time so the checks are not
             * impactful on the CPU. The client however can check every tick
             * since it's simple math. */
            if (Time.unscaledTime - _lastPacketTime > _remoteServerTimeoutDuration)
            {
                OnClientTimeOut?.Invoke();
                NetworkManager.Log($"Server has timed out. You can modify this feature on the ClientManager component.");
                StopConnection();
            }
        }
    }
}