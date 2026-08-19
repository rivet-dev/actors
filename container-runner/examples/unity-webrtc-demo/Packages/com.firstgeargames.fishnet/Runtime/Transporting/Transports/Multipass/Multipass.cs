#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Connection;
using FishNet.Managing;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Transporting.Multipass
{
    [AddComponentMenu("FishNet/Transport/Multipass")]
    public class Multipass : Transport
    {
        #region Types.
        public struct ClientTransportData : IEquatable<ClientTransportData>
        {
            /// <summary>
            /// Transport index this connection is on.
            /// </summary>
            public int TransportIndex;
            /// <summary>
            /// ConnectionId assigned by the transport.
            /// </summary>
            public int TransportId;
            /// <summary>
            /// Connection Id assigned by multipass. This Id is the one communicated to the NetworkManager.
            /// </summary>
            public int MultipassId;
            /// <summary>
            /// Cached hashcode for values.
            /// </summary>
            private int _hashCode;

            public ClientTransportData(int transportIndex, int transportId, int multipassId)
            {
                TransportIndex = transportIndex;
                TransportId = transportId;
                MultipassId = multipassId;
                _hashCode = (transportIndex, transportId, multipassId).GetHashCode();
            }

            public bool Equals(ClientTransportData other)
            {
                return _hashCode == other._hashCode;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// While true server actions such as starting or stopping the server will run on all transport.
        /// </summary>
        [Tooltip("While true server actions such as starting or stopping the server will run on all transport.")]
        public bool GlobalServerActions = true;
        /// <summary>
        /// </summary>
        private Transport _clientTransport;
        /// <summary>
        /// Transport the client is using.
        /// Use SetClientTransport to assign this value.
        /// </summary>
        [HideInInspector]
        public Transport ClientTransport
        {
            get
            {
                // If not yet set.
                if (_clientTransport == null)
                {
                    // If there are transports to set from.
                    if (_transports.Count != 0)
                        _clientTransport = _transports[0];

                    /* Give feedback to developer that transport was not set
                     * before accessing this. Transport should always be set
                     * manually rather than assuming the default client
                     * transport. */
                    if (_clientTransport == null)
                        NetworkManager.LogError($"ClientTransport in Multipass could not be set to the first transport. This can occur if no trnasports are specified or if the first entry is null.");
                    else
                        NetworkManager.LogError($"ClientTransport in Multipass is being automatically set to {_clientTransport.GetType()}. For production use SetClientTransport before attempting to access the ClientTransport.");
                }

                return _clientTransport;
            }
            private set => _clientTransport = value;
        }
        #endregion

        #region Serialized.
        /// <summary>
        /// </summary>
        [Tooltip("Transports to use.")]
        [SerializeField]
        private List<Transport> _transports = new();
        /// <summary>
        /// Transports to use.
        /// </summary>
        public IReadOnlyList<Transport> Transports => _transports;
        #endregion

        #region Private.
        /// <summary>
        /// An unset/invalid ClientTransportData.
        /// </summary>
        private readonly ClientTransportData INVALID_CLIENTTRANSPORTDATA = new(int.MinValue, int.MinValue, int.MinValue);
        /// <summary>
        /// MultipassId lookup.
        /// </summary>
        private Dictionary<int, ClientTransportData> _multpassIdLookup = new();
        /// <summary>
        /// TransportId lookup. Each index within the list is the same as the transport index.
        /// </summary>
        private List<Dictionary<int, ClientTransportData>> _transportIdLookup = new();
        /// <summary>
        /// Ids available to new connections.
        /// </summary>
        private Queue<int> _availableMultipassIds = new();
        /// <summary>
        /// Last Id added to availableMultipassIds.
        /// </summary>
        private int _lastAvailableMultipassId = 0;
        #endregion

        public override void Initialize(NetworkManager networkManager, int transportIndex)
        {
            base.Initialize(networkManager, transportIndex);

            // Remove any null transports and warn.
            for (int i = 0; i < _transports.Count; i++)
            {
                if (_transports[i] == null)
                {
                    NetworkManager.LogWarning($"Transports contains a null entry on index {i}.");
                    _transports.RemoveAt(i);
                    i--;
                }
            }

            // No transports to use.
            if (_transports.Count == 0)
            {
                NetworkManager.LogError($"No transports are set within Multipass.");
                return;
            }

            // Create transportsToMultipass.
            for (int i = 0; i < _transports.Count; i++)
            {
                Dictionary<int, ClientTransportData> dict = new();
                _transportIdLookup.Add(dict);
                // Initialize transports and callbacks.
                _transports[i].Initialize(networkManager, i);
                _transports[i].OnClientConnectionState += Multipass_OnClientConnectionState;
                _transports[i].OnServerConnectionState += Multipass_OnServerConnectionState;
                _transports[i].OnRemoteConnectionState += Multipass_OnRemoteConnectionState;
                _transports[i].OnClientReceivedData += Multipass_OnClientReceivedData;
                _transports[i].OnServerReceivedData += Multipass_OnServerReceivedData;
            }
        }

        private void OnDestroy()
        {
            // Initialize each transport.
            foreach (Transport t in _transports)
                t.Shutdown();

            ResetLookupCollections();
        }

        #region ClientIds.
        /// <summary>
        /// Resets lookup collections and caches potential garbage.
        /// </summary>
        private void ResetLookupCollections()
        {
            _multpassIdLookup.Clear();

            for (int i = 0; i < _transportIdLookup.Count; i++)
                _transportIdLookup[i].Clear();
        }

        /// <summary>
        /// Clears ClientIds when appropriate.
        /// </summary>
        private void TryResetClientIds(bool force)
        {
            // Can only clear when every transport server isnt connected.
            if (!force)
            {
                foreach (Transport t in _transports)
                {
                    // Cannot clear if a server is running still.
                    if (t.GetConnectionState(true) == LocalConnectionState.Started)
                        return;
                }
            }

            ResetLookupCollections();
            CreateAvailableIds(true);
        }

        /// <summary>
        /// Gets the Multipass connectionId using a transport connectionid.
        /// </summary>
        private ClientTransportData GetDataFromTransportId(int transportIndex, int transportId, bool log)
        {
            Dictionary<int, ClientTransportData> dict = _transportIdLookup[transportIndex];
            if (dict.TryGetValueIL2CPP(transportId, out ClientTransportData ctd))
                return ctd;

            // Fall through/fail.
            if (log)
                NetworkManager.LogError($"Multipass connectionId could not be found for transportIndex {transportIndex}, transportId of {transportId}.");
            return INVALID_CLIENTTRANSPORTDATA;
        }

        /// <summary>
        /// Gets the TransportIdData using a Multipass connectionId.
        /// </summary>
        private ClientTransportData GetDataFromMultipassId(int multipassId)
        {
            if (_multpassIdLookup.TryGetValueIL2CPP(multipassId, out ClientTransportData ctd))
                return ctd;

            // Fall through/fail.
            NetworkManager.LogError($"TransportIdData could not be found for Multipass connectionId of {multipassId}.");
            return INVALID_CLIENTTRANSPORTDATA;
        }
        #endregion

        #region ConnectionStates.
        /// <summary>
        /// Gets the IP address of a remote connectionId.
        /// </summary>
        public override string GetConnectionAddress(int multipassId)
        {
            ClientTransportData ctd = GetDataFromMultipassId(multipassId);
            if (ctd.Equals(INVALID_CLIENTTRANSPORTDATA))
                return string.Empty;

            return _transports[ctd.TransportIndex].GetConnectionAddress(ctd.TransportId);
        }

        /// <summary>
        /// Called when a connection state changes for the local client.
        /// </summary>
        public override event Action<ClientConnectionStateArgs> OnClientConnectionState;
        /// <summary>
        /// Called when a connection state changes for the local server.
        /// </summary>
        public override event Action<ServerConnectionStateArgs> OnServerConnectionState;
        /// <summary>
        /// Called when a connection state changes for a remote client.
        /// </summary>
        public override event Action<RemoteConnectionStateArgs> OnRemoteConnectionState;

        /// <summary>
        /// Gets the current local ConnectionState of the first transport.
        /// </summary>
        /// <param name = "server">True if getting ConnectionState for the server.</param>
        public override LocalConnectionState GetConnectionState(bool server)
        {
            if (server)
            {
                NetworkManager.LogError($"This method is not supported for server. Use GetConnectionState(server, transportIndex) instead.");
                return LocalConnectionState.Stopped;
            }

            if (IsClientTransportSetWithError("GetConnectionState"))
                return GetConnectionState(server, ClientTransport.Index);
            else
                return LocalConnectionState.Stopped;
        }

        /// <summary>
        /// Gets the current local ConnectionState of the transport on index.
        /// </summary>
        /// <param name = "server">True if getting ConnectionState for the server.</param>
        public LocalConnectionState GetConnectionState(bool server, int transportIndex)
        {
            if (!IndexInRange(transportIndex, true))
                return LocalConnectionState.Stopped;

            return _transports[transportIndex].GetConnectionState(server);
        }

        /// <summary>
        /// Gets the current ConnectionState of a remote client on the server.
        /// </summary>
        /// <param name = "multipassId">ConnectionId to get ConnectionState for.</param>
        public override RemoteConnectionState GetConnectionState(int multipassId)
        {
            ClientTransportData ctd = GetDataFromMultipassId(multipassId);
            if (ctd.Equals(INVALID_CLIENTTRANSPORTDATA))
                return RemoteConnectionState.Stopped;

            return _transports[ctd.TransportIndex].GetConnectionState(ctd.TransportId);
        }

        /// <summary>
        /// Gets the current ConnectionState of a remote client on the server of the transport on index.
        /// </summary>
        /// <param name = "connectionId">ConnectionId to get ConnectionState for.</param>
        public RemoteConnectionState GetConnectionState(int connectionId, int index)
        {
            if (!IndexInRange(index, true))
                return RemoteConnectionState.Stopped;

            return _transports[index].GetConnectionState(connectionId);
        }

        /// <summary>
        /// Handles a ConnectionStateArgs for the local client.
        /// </summary>
        /// <param name = "connectionStateArgs"></param>
        private void Multipass_OnClientConnectionState(ClientConnectionStateArgs connectionStateArgs)
        {
            OnClientConnectionState?.Invoke(connectionStateArgs);
        }

        /// <summary>
        /// Handles a ConnectionStateArgs for the local server.
        /// </summary>
        /// <param name = "connectionStateArgs"></param>
        private void Multipass_OnServerConnectionState(ServerConnectionStateArgs connectionStateArgs)
        {
            OnServerConnectionState?.Invoke(connectionStateArgs);
            TryResetClientIds(false);
        }

        /// <summary>
        /// Handles a ConnectionStateArgs for a remote client.
        /// </summary>
        /// <param name = "connectionStateArgs"></param>
        private void Multipass_OnRemoteConnectionState(RemoteConnectionStateArgs connectionStateArgs)
        {
            /* When starting Multipass needs to get a new
             * connectionId to be used within FN. This is the 'ClientId'
             * that is passed around for ownership, rpcs, ect.
             *
             * The new connectionId will be linked with the connectionId
             * from the transport, named transportConnectionid.
             *
             * When data arrives the transportStateId is used as a key
             * in fromClientIds, where Multipass Id is returned. The argument values
             * are then overwritten with the MultipassId.
             *
             * When data is being sent the same process is performed but reversed.
             * The connectionId is looked up in toClientIds, where the transportConnectionId
             * is output. Then as before the argument values are overwritten with the
             * transportConnectionId. */

            int transportIndex = connectionStateArgs.TransportIndex;
            int transportConnectionId = connectionStateArgs.ConnectionId;
            /* MultipassId is set to a new value when connecting
             * or discovered value when disconnecting. */
            int multipassId;
            Dictionary<int, ClientTransportData> transportToMultipass = _transportIdLookup[transportIndex];

            // Started.
            if (connectionStateArgs.ConnectionState == RemoteConnectionState.Started)
            {
                if (_availableMultipassIds.Count == 0)
                {
                    bool addedIds = CreateAvailableIds(false);
                    if (!addedIds)
                    {
                        NetworkManager.Log($"There are no more available connectionIds to use. Connection {transportConnectionId} has been kicked.");
                        _transports[transportIndex].StopConnection(transportConnectionId, true);
                        return;
                    }
                }
                // Get a multipassId for new connections.
                multipassId = _availableMultipassIds.Dequeue();

                // Get and update a clienttransportdata.
                ClientTransportData ctd = new(transportIndex, transportConnectionId, multipassId);
                // Assign the lookup for transportId/index.
                transportToMultipass[transportConnectionId] = ctd;
                // Assign the lookup for multipassId.
                _multpassIdLookup[multipassId] = ctd;

                // Update args to use multipassId before invoking.
                connectionStateArgs.ConnectionId = multipassId;
                OnRemoteConnectionState?.Invoke(connectionStateArgs);
            }
            // Stopped.
            else
            {
                Transport transport = _transports[transportIndex];
                // Only log if the server is started for the specified transport.
                bool log = transport.GetConnectionState(server: true) == LocalConnectionState.Started;

                ClientTransportData ctd = GetDataFromTransportId(transportIndex, transportConnectionId, log);
                /* If CTD could not be found then the connection
                 * is not stored/known. Nothing further can be done; the event cannot
                 * invoke either since Id is unknown. */
                if (ctd.Equals(INVALID_CLIENTTRANSPORTDATA))
                    return;

                // Add the multipassId back to the queue.
                _availableMultipassIds.Enqueue(ctd.MultipassId);
                transportToMultipass.Remove(transportConnectionId);
                _multpassIdLookup.Remove(ctd.MultipassId);
#if DEVELOPMENT
                // Remove packets held for connection from latency simulator.
                NetworkManager.TransportManager.LatencySimulator.RemovePendingForConnection(ctd.MultipassId);
#endif

                // Update args to use multipassId before invoking.
                connectionStateArgs.ConnectionId = ctd.MultipassId;
                OnRemoteConnectionState?.Invoke(connectionStateArgs);
            }
        }
        #endregion

        #region Iterating.
        /// <summary>
        /// Processes data received by the socket.
        /// </summary>
        /// <param name = "asServer">True to process data received on the server.</param>
        public override void IterateIncoming(bool asServer)
        {
            foreach (Transport t in _transports)
                t.IterateIncoming(asServer);
        }

        /// <summary>
        /// Processes data to be sent by the socket.
        /// </summary>
        /// <param name = "asServer">True to send data from the local server to clients, false to send from the local client to server.
        public override void IterateOutgoing(bool asServer)
        {
            foreach (Transport t in _transports)
                t.IterateOutgoing(asServer);
        }
        #endregion

        #region ReceivedData.
        /// <summary>
        /// Called when client receives data.
        /// </summary>
        public override event Action<ClientReceivedDataArgs> OnClientReceivedData;

        /// <summary>
        /// Handles a ClientReceivedDataArgs.
        /// </summary>
        /// <param name = "receivedDataArgs"></param>
        private void Multipass_OnClientReceivedData(ClientReceivedDataArgs receivedDataArgs)
        {
            OnClientReceivedData?.Invoke(receivedDataArgs);
        }

        /// <summary>
        /// Called when server receives data.
        /// </summary>
        public override event Action<ServerReceivedDataArgs> OnServerReceivedData;

        /// <summary>
        /// Handles a ClientReceivedDataArgs.
        /// </summary>
        /// <param name = "receivedDataArgs"></param>
        private void Multipass_OnServerReceivedData(ServerReceivedDataArgs receivedDataArgs)
        {
            ClientTransportData ctd = GetDataFromTransportId(receivedDataArgs.TransportIndex, receivedDataArgs.ConnectionId, log: true);
            if (ctd.Equals(INVALID_CLIENTTRANSPORTDATA))
                return;

            receivedDataArgs.ConnectionId = ctd.MultipassId;
            OnServerReceivedData?.Invoke(receivedDataArgs);
        }
        #endregion

        #region Sending.
        /// <summary>
        /// Sends to the server on ClientTransport.
        /// </summary>
        /// <param name = "channelId">Channel to use.</param>
        /// ///
        /// <param name = "segment">Data to send.</param>
        public override void SendToServer(byte channelId, ArraySegment<byte> segment)
        {
            if (ClientTransport != null)
                ClientTransport.SendToServer(channelId, segment);
        }

        /// <summary>
        /// Sends data to a client.
        /// </summary>
        public override void SendToClient(byte channelId, ArraySegment<byte> segment, int multipassId)
        {
            ClientTransportData ctd = GetDataFromMultipassId(multipassId);
            if (ctd.Equals(INVALID_CLIENTTRANSPORTDATA))
                return;

            _transports[ctd.TransportIndex].SendToClient(channelId, segment, ctd.TransportId);
        }

        /// <summary>
        /// Sends data to a client.
        /// </summary>
        /// <param name = "transportIndex">TransportIndex the client is using.</param>
        public void SendToClient(byte channelId, ArraySegment<byte> segment, int transportId, int transportIndex)
        {
            _transports[transportIndex].SendToClient(channelId, segment, transportId);
        }
        #endregion

        #region Configuration.
        /// <summary>
        /// Returns if GlobalServerActions is true and if not logs an error.
        /// </summary>
        /// <returns></returns>
        private bool UseGlobalServerActionsWithError(string methodText)
        {
            if (!GlobalServerActions)
            {
                NetworkManager.LogError($"Method {methodText} is not supported while GlobalServerActions is false.");
                return false;
            }
            else
            {
                return true;
            }
        }

        /// <summary>
        /// Returns if ClientTransport is set and if not logs an error.
        /// </summary>
        /// <param name = "methodText"></param>
        /// <returns></returns>
        private bool IsClientTransportSetWithError(string methodText)
        {
            if (ClientTransport == null)
            {
                NetworkManager.LogError($"ClientTransport is not set. Use SetClientTransport before calling {methodText}.");
                return false;
            }
            else
            {
                return true;
            }
        }

        /// <summary>
        /// Populates the availableIds collection.
        /// </summary>
        /// <returns>True if at least 1 Id was added.</returns>
        private bool CreateAvailableIds(bool reset)
        {
            if (reset)
            {
                _lastAvailableMultipassId = 0;
                _availableMultipassIds.Clear();
            }
            // Add in blocks of 1000.
            int added = 0;
            while (_lastAvailableMultipassId <= NetworkConnection.MAXIMUM_CLIENTID_WITHOUT_SIMULATED_VALUE && added < 1000)
            {
                added++;
                _availableMultipassIds.Enqueue(_lastAvailableMultipassId);
                _lastAvailableMultipassId++;
            }

            return added > 0;
        }

        /// <summary>
        /// Sets the client transport to the first of type.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        public void SetClientTransport<T>()
        {
            int index = -1;
            for (int i = 0; i < _transports.Count; i++)
            {
                if (_transports[i].GetType() == typeof(T))
                {
                    index = i;
                    break;
                }
            }

            SetClientTransport(index);
        }

        /// <summary>
        /// Sets the client transport to the first of type T.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        public void SetClientTransport(Type type)
        {
            int index = -1;
            for (int i = 0; i < _transports.Count; i++)
            {
                if (_transports[i].GetType() == type)
                {
                    index = i;
                    break;
                }
            }

            SetClientTransport(index);
        }

        /// <summary>
        /// Sets the client transport to the matching reference of transport.
        /// </summary>
        /// <param name = "transport"></param>
        public void SetClientTransport(Transport transport)
        {
            int index = -1;
            for (int i = 0; i < _transports.Count; i++)
            {
                if (_transports[i] == transport)
                {
                    index = i;
                    break;
                }
            }

            SetClientTransport(index);
        }

        /// <summary>
        /// Sets the client transport to the transport on index.
        /// </summary>
        /// <param name = "index"></param>
        public void SetClientTransport(int index)
        {
            if (!IndexInRange(index, true))
                return;

            ClientTransport = _transports[index];
        }

        /// <summary>
        /// Gets the Transport on index.
        /// </summary>
        /// <param name = "index"></param>
        /// <returns></returns>
        public Transport GetTransport(int index)
        {
            if (!IndexInRange(index, true))
                return null;

            return _transports[index];
        }

        /// <summary>
        /// Gets the Transport on of type T.
        /// </summary>
        /// <param name = "index"></param>
        /// <returns></returns>
        public T GetTransport<T>()
        {
            foreach (Transport t in _transports)
            {
                if (t.GetType() == typeof(T))
                    return (T)(object)t;
            }

            return default;
        }

        /// <summary>
        /// Returns if the first transport is a local transport, optionally checking against connectionId.
        /// While true several security checks are disabled.
        /// </summary>
        public override bool IsLocalTransport(int connectionId)
        {
            foreach (Transport item in _transports)
                return item.IsLocalTransport(connectionId);

            return false;
        }

        /// <summary>
        /// Returns if the transportId is a local transport, optionally checking against connectionId.
        /// While true several security checks are disabled.
        /// </summary>
        public bool IsLocalTransport(int transportId, int connectionId)
        {
            if (!IndexInRange(transportId, true))
                return false;

            return _transports[transportId].IsLocalTransport(connectionId);
        }

        /// <summary>
        /// Returns the maximum number of clients allowed to connect to the server. If the transport does not support this method the value -1 is returned.
        /// This method is not supported. Use GetMaximumClients(transportIndex) instead.
        /// </summary>
        /// <returns></returns>
        public override int GetMaximumClients()
        {
            NetworkManager.LogError($"This method is not supported. Use GetMaximumClients(transportIndex) instead.");
            return -1;
        }

        /// <summary>
        /// Returns the maximum number of clients allowed to connect to the server. If the transport does not support this method the value -1 is returned.
        /// The first transport is used.
        /// </summary>
        /// <returns></returns>
        public int GetMaximumClients(int transportIndex)
        {
            if (!IndexInRange(transportIndex, true))
                return -1;

            return _transports[transportIndex].GetMaximumClients();
        }

        /// <summary>
        /// Sets maximum number of clients allowed to connect to the server. If applied at runtime and clients exceed this value existing clients will stay connected but new clients may not connect.
        /// This sets the value for every transport.
        /// </summary>
        /// <param name = "value"></param>
        public override void SetMaximumClients(int value)
        {
            foreach (Transport t in _transports)
                t.SetMaximumClients(value);
        }

        /// <summary>
        /// Sets maximum number of clients allowed to connect to the server. If applied at runtime and clients exceed this value existing clients will stay connected but new clients may not connect.
        /// This sets the value to the transport on index.
        /// </summary>
        /// <param name = "value"></param>
        public void SetMaximumClients(int value, int transportIndex)
        {
            if (!IndexInRange(transportIndex, true))
                return;

            _transports[transportIndex].SetMaximumClients(value);
        }

        /// <summary>
        /// Sets which address the client will connect to.
        /// This will set the address for every transport.
        /// </summary>
        /// <param name = "address"></param>
        public override void SetClientAddress(string address)
        {
            foreach (Transport t in _transports)
                t.SetClientAddress(address);
        }

        /// <summary>
        /// Sets which address the client will connect to.
        /// </summary>
        /// <param name = "address"></param>
        /// <param name = "index">Transport index to set for.</param>
        public void SetClientAddress(string address, int index)
        {
            if (!IndexInRange(index, true))
                return;

            _transports[index].SetClientAddress(address);
        }

        /// <summary>
        /// Sets which address the server will bind to.
        /// This will set the address for every transport.
        /// </summary>
        public override void SetServerBindAddress(string address, IPAddressType addressType)
        {
            foreach (Transport t in _transports)
                t.SetServerBindAddress(address, addressType);
        }

        /// Sets which address the server will bind to.
        /// This is called on the transport of index.
        /// </summary>
        /// <param name = "address"></param>
        public void SetServerBindAddress(string address, IPAddressType addressType, int index)
        {
            if (!IndexInRange(index, true))
                return;

            _transports[index].SetServerBindAddress(address, addressType);
        }

        /// <summary>
        /// Sets which port to use.
        /// This will set the port for every transport.
        /// </summary>
        public override void SetPort(ushort port)
        {
            foreach (Transport t in _transports)
                t.SetPort(port);
        }

        /// <summary>
        /// Sets which port to use on transport of index.
        /// </summary>
        public void SetPort(ushort port, int index)
        {
            if (!IndexInRange(index, true))
                return;

            _transports[index].SetPort(port);
        }

        /// <summary>
        /// Gets the first transports port.
        /// </summary>
        /// <returns></returns>
        public override ushort GetPort()
        {
            foreach (Transport t in _transports)
                return t.GetPort();

            return base.GetPort();
        }
        #endregion

        #region Start and stop.
        /// <summary>
        /// Starts the local server or client using configured settings on the first transport.
        /// </summary>
        /// <param name = "server">True to start server.</param>
        public override bool StartConnection(bool server)
        {
            // Server.
            if (server)
            {
                if (!UseGlobalServerActionsWithError("StartConnection"))
                    return false;

                bool success = true;
                for (int i = 0; i < _transports.Count; i++)
                {
                    if (!StartConnection(true, i))
                        success = false;
                }

                return success;
            }
            // Client.
            else
            {
                if (IsClientTransportSetWithError("StartConnection"))
                    return StartConnection(false, ClientTransport.Index);
                else
                    return false;
            }
        }

        /// <summary>
        /// Starts the local server or client using configured settings on transport of index.
        /// </summary>
        /// <param name = "server">True to start server.</param>
        public bool StartConnection(bool server, int index)
        {
            if (server)
            {
                return StartServer(index);
            }
            else
            {
                if (IsClientTransportSetWithError("StartConnection"))
                    return StartClient();
                else
                    return false;
            }
        }

        /// <summary>
        /// Stops the local server or client on the first transport.
        /// </summary>
        /// <param name = "server">True to stop server.</param>
        public override bool StopConnection(bool server)
        {
            // Server
            if (server)
            {
                if (!UseGlobalServerActionsWithError("StopConnection"))
                    return false;

                bool success = true;
                for (int i = 0; i < _transports.Count; i++)
                {
                    if (!StopConnection(true, i))
                        success = false;
                }

                return success;
            }
            // Client.
            else
            {
                if (IsClientTransportSetWithError("StopConnection"))
                    return StopConnection(false, ClientTransport.Index);
                else
                    return false;
            }
        }

        /// <summary>
        /// Stops the local server or client on transport of index.
        /// </summary>
        /// <param name = "server">True to stop server.</param>
        public bool StopConnection(bool server, int index)
        {
            if (server)
            {
                return StopServer(index);
            }
            else
            {
                if (IsClientTransportSetWithError("StopConnection"))
                    return StopClient();
                else
                    return false;
            }
        }

        /// <summary>
        /// Stops a remote client from the server, disconnecting the client.
        /// </summary>
        /// <param name = "connectionId">ConnectionId of the client to disconnect.</param>
        /// <param name = "immediately">True to abrutly stp the client socket without waiting socket thread.</param>
        public override bool StopConnection(int connectionId, bool immediately)
        {
            return StopClient(connectionId, immediately);
        }

        /// <summary>
        /// Stops the server connection on transportIndex.
        /// </summary>
        /// <param name = "sendDisconnectMessage">True to send a disconnect message to connections before stopping them.</param>
        /// <param name = "transportIndex">Index of transport to stop on.</param>
        public bool StopServerConnection(bool sendDisconnectMessage, int transportIndex)
        {
            if (sendDisconnectMessage)
            {
                // Get dictionary for transportIndex.
                Dictionary<int, ClientTransportData> dict = _transportIdLookup[transportIndex];
                // Create an array containing all multipass Ids for transportIndex.
                int[] multipassIds = new int[dict.Count];
                int index = 0;
                foreach (ClientTransportData item in dict.Values)
                    multipassIds[index++] = item.MultipassId;
                // Tell serve manager to write disconnect for those ids.
                NetworkManager.ServerManager.SendDisconnectMessages(multipassIds);
                // Iterate outgoing on transport which is being stopped.
                _transports[transportIndex].IterateOutgoing(asServer: true);
            }

            return StopConnection(true, transportIndex);
        }

        /// <summary>
        /// Stops both client and server on all transports.
        /// </summary>
        public override void Shutdown()
        {
            foreach (Transport t in _transports)
            {
                // Stops client then server connections.
                t.StopConnection(false);
                t.StopConnection(true);
            }
        }

        #region Privates.
        /// <summary>
        /// Starts server of transport on index.
        /// </summary>
        /// <returns>True if there were no blocks. A true response does not promise a socket will or has connected.</returns>
        private bool StartServer(int index)
        {
            if (!IndexInRange(index, true))
                return false;

            return _transports[index].StartConnection(true);
        }

        /// <summary>
        /// Stops server of transport on index.
        /// </summary>
        private bool StopServer(int index)
        {
            if (!IndexInRange(index, true))
                return false;

            return _transports[index].StopConnection(true);
        }

        /// <summary>
        /// Starts the client on ClientTransport.
        /// </summary>
        /// <param name = "address"></param>
        /// <returns>True if there were no blocks. A true response does not promise a socket will or has connected.</returns>
        private bool StartClient()
        {
            return ClientTransport.StartConnection(false);
        }

        /// <summary>
        /// Stops the client on ClientTransport.
        /// </summary>
        private bool StopClient()
        {
            return ClientTransport.StopConnection(false);
        }

        /// <summary>
        /// Stops a remote client on the server.
        /// </summary>
        /// <param name = "multipassId"></param>
        /// <param name = "immediately">True to abrutly stp the client socket without waiting socket thread.</param>
        private bool StopClient(int multipassId, bool immediately)
        {
            ClientTransportData ctd = GetDataFromMultipassId(multipassId);
            if (ctd.Equals(INVALID_CLIENTTRANSPORTDATA))
                return false;

            return _transports[ctd.TransportIndex].StopConnection(ctd.TransportId, immediately);
        }
        #endregion
        #endregion

        #region Channels.
        /// <summary>
        /// Gets the MTU for a channel on the first transport. This should take header size into consideration.
        /// For example, if MTU is 1200 and a packet header for this channel is 10 in size, this method should return 1190.
        /// </summary>
        /// <param name = "channel"></param>
        /// <returns></returns>
        public override int GetMTU(byte channel)
        {
            return GetMTU(channel, 0);
        }

        /// <summary>
        /// Gets the MTU for a channel of transport on index. This should take header size into consideration.
        /// For example, if MTU is 1200 and a packet header for this channel is 10 in size, this method should return 1190.
        /// </summary>
        /// <param name = "channel"></param>
        /// <returns></returns>
        public int GetMTU(byte channel, int index)
        {
            if (!IndexInRange(index, true))
                return -1;

            return _transports[index].GetMTU(channel);
        }
        #endregion

        #region Misc.
        /// <summary>
        /// Returns if an index is within range of the Transports collection.
        /// </summary>
        private bool IndexInRange(int index, bool error)
        {
            if (index >= _transports.Count || index < 0)
            {
                if (error)
                    NetworkManager.LogError($"Index of {index} is out of Transports range.");
                return false;
            }
            else
            {
                return true;
            }
        }

        // perf change events to direct calls in transports.
        public override void HandleServerConnectionState(ServerConnectionStateArgs connectionStateArgs) { }
        public override void HandleRemoteConnectionState(RemoteConnectionStateArgs connectionStateArgs) { }
        public override void HandleClientReceivedDataArgs(ClientReceivedDataArgs receivedDataArgs) { }
        public override void HandleServerReceivedDataArgs(ServerReceivedDataArgs receivedDataArgs) { }
        public override void HandleClientConnectionState(ClientConnectionStateArgs connectionStateArgs) { }
        #endregion
    }
}