using System;
using FishNet.CodeGenerating;
using FishNet.Managing;
using FishNet.Managing.Timing;
using FishNet.Serializing;
using FishNet.Transporting;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Object.Synchronizing.Internal
{
    public class SyncBase
    {
        #region Public.
        /// <summary>
        /// True if this SyncBase has been initialized on its NetworkBehaviour.
        /// Being true does not mean that the NetworkBehaviour has been initialized on the network, but rather that this SyncBase has been configured with the basics to be networked.
        /// </summary>
        public bool IsInitialized { get; private set; }
        /// <summary>
        /// True if the object for which this SyncType is for has been initialized for the network.
        /// </summary>
        public bool IsNetworkInitialized => IsInitialized && (NetworkBehaviour.IsServerInitialized || NetworkBehaviour.IsClientInitialized);
        /// <summary>
        /// True if a SyncObject, false if a SyncVar.
        /// </summary>
        public bool IsSyncObject { get; private set; }
        /// <summary>
        /// The settings for this SyncVar.
        /// </summary>
        [MakePublic]
        internal SyncTypeSettings Settings;
        /// <summary>
        /// How often updates may send.
        /// </summary>
        [MakePublic]
        internal float SendRate => Settings.SendRate;
        /// <summary>
        /// True if this SyncVar needs to send data.
        /// </summary>
        public bool IsDirty { get; private set; }
        /// <summary>
        /// NetworkManager this uses.
        /// </summary>
        public NetworkManager NetworkManager = null;
        /// <summary>
        /// NetworkBehaviour this SyncVar belongs to.
        /// </summary>
        public NetworkBehaviour NetworkBehaviour = null;
        /// <summary>
        /// True if the server side has initialized this SyncType.
        /// </summary>
        public bool OnStartServerCalled { get; private set; }
        /// <summary>
        /// True if the client side has initialized this SyncType.
        /// </summary>
        public bool OnStartClientCalled { get; private set; }
        /// <summary>
        /// Next time this SyncType may send data.
        /// This is also the next time a client may send to the server when using client-authoritative SyncTypes.
        /// </summary>
        [MakePublic]
        internal uint NextSyncTick = 0;
        /// <summary>
        /// Index within the sync collection.
        /// </summary>
        public uint SyncIndex { get; protected set; } = 0;
        /// <summary>
        /// Channel to send on.
        /// </summary>
        internal Channel Channel => _currentChannel;

        /// <summary>
        /// Sets a new currentChannel.
        /// </summary>
        /// <param name = "channel"></param>
        internal void SetCurrentChannel(Channel channel) => _currentChannel = channel;
        #endregion

        #region Private.
        /// <summary>
        /// Sync interval converted to ticks.
        /// </summary>
        private uint _timeToTicks;
        /// <summary>
        /// Channel to use for next write. To ensure eventual consistency this eventually changes to reliable when Settings are unreliable.
        /// </summary>
        private Channel _currentChannel;
        /// <summary>
        /// Last changerId read from sender.
        /// </summary>
        private ushort _lastReadChangeId = UNSET_CHANGE_ID;
        /// <summary>
        /// Last changeId that was sent to receivers.
        /// </summary>
        private ushort _lastWrittenChangeId = UNSET_CHANGE_ID;
        #endregion

        #region Consts.
        /// <summary>
        /// Value to use when readId is unset.
        /// </summary>
        private const ushort UNSET_CHANGE_ID = 0;
        /// <summary>
        /// Maximum value readId can be before resetting to the beginning.
        /// </summary>
        private const ushort MAXIMUM_CHANGE_ID = ushort.MaxValue;
        #endregion

        #region Constructors
        public SyncBase() : this(new()) { }

        public SyncBase(SyncTypeSettings settings)
        {
            Settings = settings;
        }
        #endregion

        /// <summary>
        /// Updates settings with new values.
        /// </summary>
        public void UpdateSettings(SyncTypeSettings settings)
        {
            Settings = settings;
            SetTimeToTicks();
        }

        /// <summary>
        /// Updates settings with new values.
        /// </summary>
        public void UpdatePermissions(WritePermission writePermissions, ReadPermission readPermissions)
        {
            UpdatePermissions(writePermissions);
            UpdatePermissions(readPermissions);
        }

        /// <summary>
        /// Updates settings with new values.
        /// </summary>
        public void UpdatePermissions(WritePermission writePermissions) => Settings.WritePermission = writePermissions;

        /// <summary>
        /// Updates settings with new values.
        /// </summary>
        public void UpdatePermissions(ReadPermission readPermissions) => Settings.ReadPermission = readPermissions;

        /// <summary>
        /// Updates settings with new values.
        /// </summary>
        public void UpdateSendRate(float sendRate)
        {
            Settings.SendRate = sendRate;
            SetTimeToTicks();
        }

        /// <summary>
        /// Updates settings with new values.
        /// </summary>
        public void UpdateSettings(Channel channel)
        {
            CheckChannel(ref channel);
            _currentChannel = channel;
        }

        /// <summary>
        /// Updates settings with new values.
        /// </summary>
        public void UpdateSettings(WritePermission writePermissions, ReadPermission readPermissions, float sendRate, Channel channel)
        {
            CheckChannel(ref channel);
            _currentChannel = channel;
            Settings = new(writePermissions, readPermissions, sendRate, channel);
            SetTimeToTicks();
        }

        /// <summary>
        /// Checks channel and corrects if not valid.
        /// </summary>
        /// <param name = "c"></param>
        private void CheckChannel(ref Channel c)
        {
            if (c == Channel.Unreliable && IsSyncObject)
            {
                c = Channel.Reliable;
                string warning = $"Channel cannot be unreliable for SyncObjects. Channel has been changed to reliable.";
                NetworkManager.LogWarning(warning);
            }
        }

        /// <summary>
        /// Initializes this SyncBase before user Awake code.
        /// </summary>
        [MakePublic]
        internal void InitializeEarly(NetworkBehaviour nb, uint syncIndex, bool isSyncObject)
        {
            NetworkBehaviour = nb;
            SyncIndex = syncIndex;
            IsSyncObject = isSyncObject;

            NetworkBehaviour.RegisterSyncType(this, SyncIndex);
        }

        /// <summary>
        /// Called during InitializeLate in NetworkBehaviours to indicate user Awake code has executed.
        /// </summary>
        [MakePublic]
        internal void InitializeLate()
        {
            Initialized();
        }

        /// <summary>
        /// Called when the SyncType has been registered, but not yet initialized over the network.
        /// </summary>
        protected virtual void Initialized()
        {
            IsInitialized = true;
        }

        /// <summary>
        /// PreInitializes this for use with the network.
        /// </summary>
        [MakePublic]
        protected internal void PreInitialize(NetworkManager networkManager, bool asServer)
        {
            NetworkManager = networkManager;

            if (Settings.IsDefault())
            {
                float sendRate = Mathf.Max(networkManager.ServerManager.GetSyncTypeRate(), (float)networkManager.TimeManager.TickDelta);
                Settings = new(sendRate);
            }

            SetTimeToTicks();
        }

        /// <summary>
        /// Sets ticks needed to pass for send rate.
        /// </summary>
        private void SetTimeToTicks()
        {
            if (NetworkManager == null)
                return;
            _timeToTicks = NetworkManager.TimeManager.TimeToTicks(Settings.SendRate, TickRounding.RoundUp);
        }

        /// <summary>
        /// Called after OnStartXXXX has occurred for the NetworkBehaviour.
        /// </summary>
        /// <param name = "asServer">True if OnStartServer was called, false if OnStartClient.</param>
        [MakePublic]
        protected internal virtual void OnStartCallback(bool asServer)
        {
            if (asServer)
                OnStartServerCalled = true;
            else
                OnStartClientCalled = true;
        }

        /// <summary>
        /// Called before OnStopXXXX has occurred for the NetworkBehaviour.
        /// </summary>
        /// <param name = "asServer">True if OnStopServer was called, false if OnStopClient.</param>
        [MakePublic]
        protected internal virtual void OnStopCallback(bool asServer)
        {
            if (asServer)
                OnStartServerCalled = false;
            else
                OnStartClientCalled = false;
        }

        /// <summary>
        /// True if can set values and send them over the network.
        /// </summary>
        /// <param name = "log"></param>
        /// <returns></returns>
        protected bool CanNetworkSetValues(bool log = true)
        {
            /* If not registered then values can be set
             * since at this point the object is still being initialized
             * in awake so we want those values to be applied. */
            if (!IsInitialized)
                return true;
            /* If the network is not initialized yet then let
             * values be set. Values set here will not synchronize
             * to the network. We are assuming the user is setting
             * these values on client and server appropriately
             * since they are being applied prior to this object
             * being networked. */
            if (!IsNetworkInitialized)
                return true;
            // If server is active then values can be set no matter what.
            if (NetworkBehaviour.IsServerStarted)
                return true;
            /* If here then server is not active and additional
             * checks must be performed. */
            bool result = Settings.WritePermission == WritePermission.ClientUnsynchronized || (Settings.ReadPermission == ReadPermission.ExcludeOwner && NetworkBehaviour.IsOwner);
            if (!result && log)
                LogServerNotActiveWarning();

            return result;
        }

        /// <summary>
        /// Logs that the operation could not be completed because the server is not active.
        /// </summary>
        protected void LogServerNotActiveWarning()
        {
            if (NetworkManager != null)
                NetworkManager.LogWarning($"Cannot complete operation as server when server is not active. You can disable this warning by setting WritePermissions to {WritePermission.ClientUnsynchronized.ToString()}.");
        }

        /// <summary>
        /// Dirties this Sync and the NetworkBehaviour.
        /// </summary>
        /// <param name = "sendRpc">True to send current dirtied values immediately as a RPC. When this occurs values will arrive in the order they are sent and interval is ignored.</param>
        protected bool Dirty() // bool sendRpc = false)
        {
            // if (sendRpc)
            //    NextSyncTick = 0;
            /* Reset channel even if already dirty.
             * This is because the value might have changed
             * which will reset the eventual consistency state. */
            _currentChannel = Settings.Channel;

            /* Once dirty don't undirty until it's
             * processed. This ensures that data
             * is flushed. */
            bool canDirty = NetworkBehaviour.DirtySyncType();
            IsDirty |= canDirty;

            return canDirty;
        }

        /// <summary>
        /// Returns if callbacks can be invoked with asServer ture.
        /// This is typically used when the value is changing through user code, causing supplier to be unknown.
        /// </summary>
        /// <returns></returns>
        protected bool CanInvokeCallbackAsServer() => !IsNetworkInitialized || NetworkBehaviour.IsServerStarted;

        /// <summary>
        /// Reads a change Id and returns true if the change is new.
        /// </summary>
        /// <remarks>This method is currently under evaluation and may change at any time.</remarks>
        protected virtual bool ReadChangeId(Reader reader)
        {
            if (NetworkManager == null)
            {
                NetworkManager.LogWarning($"NetworkManager is unexpectedly null during a SyncType read.");
                return false;
            }

            bool rolledOver = reader.ReadBoolean();
            ushort id = reader.ReadUInt16();

            // Only check lastReadId if its not unset.
            if (_lastReadChangeId != UNSET_CHANGE_ID)
            {
                /* If not rolledOver then Id should always be larger
                 * than the last read. If it's not then the data is
                 * old.
                 *
                 * If Id is smaller then rolledOver should be normal,
                 * as rolling over means to restart the Id from the lowest
                 * value. */
                if (rolledOver)
                {
                    if (id >= _lastReadChangeId)
                        return false;
                }
                else
                {
                    if (id <= _lastReadChangeId)
                        return false;
                }
            }

            _lastReadChangeId = id;
            return true;
        }

        /// <summary>
        /// Writes the readId for a change.
        /// </summary>
        /// <remarks>This method is currently under evaluation and may change at any time.</remarks>
        protected virtual void WriteChangeId(PooledWriter writer)
        {
            bool rollOver;
            if (_lastWrittenChangeId >= MAXIMUM_CHANGE_ID)
            {
                rollOver = true;
                _lastWrittenChangeId = UNSET_CHANGE_ID;
            }
            else
            {
                rollOver = false;
            }

            _lastWrittenChangeId++;
            writer.WriteBoolean(rollOver);
            writer.WriteUInt16(_lastWrittenChangeId);
        }

        /// <summary>
        /// Returns true if values are being read as clientHost.
        /// </summary>
        /// <param name = "asServer">True if reading as server.</param>
        /// <remarks>This method is currently under evaluation and may change at any time.</remarks>
        protected bool IsReadAsClientHost(bool asServer) => !asServer && NetworkManager.IsServerStarted;

        /// <summary>
        /// Returns true if values are being read as clientHost.
        /// </summary>
        /// <param name = "asServer">True if reading as server.</param>
        /// <remarks>This method is currently under evaluation and may change at any time.</remarks>
        protected bool CanReset(bool asServer)
        {
            bool clientStarted = IsNetworkInitialized && NetworkManager.IsClientStarted;
            return (asServer && !clientStarted) || (!asServer && NetworkBehaviour.IsDeinitializing);
        }

        /// <summary>
        /// Outputs values which may be helpful on how to process a read operation.
        /// </summary>
        /// <param name = "newChangeId">True if the changeId read is not old data.</param>
        /// <param name = "asClientHost">True if being read as clientHost.</param>
        /// <param name = "canModifyValues">True if can modify values from the read, typically when asServer or not asServer and not clientHost.</param>
        /// <remarks>This method is currently under evaluation and may change at any time.</remarks>
        protected void SetReadArguments(PooledReader reader, bool asServer, out bool newChangeId, out bool asClientHost, out bool canModifyValues)
        {
            newChangeId = ReadChangeId(reader);
            asClientHost = IsReadAsClientHost(asServer);
            canModifyValues = newChangeId && !asClientHost;
        }

        /// <summary>
        /// Sets IsDirty to false.
        /// </summary>
        internal void ResetDirty()
        {
            // If not a sync object and using unreliable channel.
            if (!IsSyncObject && Settings.Channel == Channel.Unreliable)
            {
                // Check if dirty can be unset or if another tick must be run using reliable.
                if (_currentChannel == Channel.Unreliable)
                    _currentChannel = Channel.Reliable;
                // Already sent reliable, can undirty. Channel will reset next time this dirties.
                else
                    IsDirty = false;
            }
            //If syncObject or using reliable unset dirty.
            else
            {
                IsDirty = false;
            }
        }

        /// <summary>
        /// True if dirty and enough time has passed to write changes.
        /// </summary>
        internal bool IsNextSyncTimeMet(uint tick) => IsDirty && tick >= NextSyncTick;

        [Obsolete("Use IsNextSyncTimeMet.")] //Remove on V5
        internal bool SyncTimeMet(uint tick) => IsNextSyncTimeMet(tick);

        /// <summary>
        /// Writes current value.
        /// </summary>
        /// <param name = "resetSyncTick">True to set the next time data may sync.</param>
        [MakePublic]
        protected internal virtual void WriteDelta(PooledWriter writer, bool resetSyncTick = true)
        {
            WriteHeader(writer, resetSyncTick);
        }

        /// <summary>
        /// Writes the header for this SyncType.
        /// </summary>
        protected virtual void WriteHeader(PooledWriter writer, bool resetSyncTick = true)
        {
            if (resetSyncTick)
                NextSyncTick = NetworkManager.TimeManager.LocalTick + _timeToTicks;

            writer.WriteUInt8Unpacked((byte)SyncIndex);
            WriteChangeId(writer);
        }

        /// <summary>
        /// Indicates that a full write has occurred.
        /// This is called from WriteFull, or can be called manually.
        /// </summary>
        [Obsolete("This method no longer functions. You may remove it from your code.")] //Remove on V5.
        protected void FullWritten() { }

        /// <summary>
        /// Writes all values for the SyncType.
        /// </summary>
        [MakePublic]
        protected internal virtual void WriteFull(PooledWriter writer) { }

        /// <summary>
        /// Sets current value as server or client through deserialization.
        /// </summary>
        [MakePublic]
        protected internal virtual void Read(PooledReader reader, bool asServer) { }

        /// <summary>
        /// Resets initialized values for server and client.
        /// </summary>
        protected internal virtual void ResetState()
        {
            ResetState(true);
            ResetState(false);
        }

        /// <summary>
        /// Resets initialized values for server or client.
        /// </summary>
        [MakePublic]
        protected internal virtual void ResetState(bool asServer)
        {
            if (asServer)
            {
                NextSyncTick = 0;
                SetCurrentChannel(Settings.Channel);
                IsDirty = false;
            }

            /* This only needs to be reset for clients, since
             * it only applies to clients. But if the server is resetting
             * that means the object is deinitializing, and won't have any
             * client observers anyway. Because of this it's safe to reset
             * with asServer true, or false.
             *
             * This change is made to resolve a bug where asServer:false
             * sometimes does not invoke when stopping clientHost while not
             * also stopping play mode. */
            _lastReadChangeId = UNSET_CHANGE_ID;
        }
    }
}