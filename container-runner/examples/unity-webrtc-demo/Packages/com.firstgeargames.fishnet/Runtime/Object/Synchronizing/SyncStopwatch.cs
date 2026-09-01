using FishNet.Documenting;
using FishNet.Object.Synchronizing.Internal;
using FishNet.Serializing;
using System.Collections.Generic;

namespace FishNet.Object.Synchronizing
{
    /// <summary>
    /// A SyncObject to efficiently synchronize Stopwatchs over the network.
    /// </summary>
    public class SyncStopwatch : SyncBase, ICustomSync
    {
        #region Type.
        /// <summary>
        /// Information about how the Stopwatch has changed.
        /// </summary>
        private struct ChangeData
        {
            public readonly SyncStopwatchOperation Operation;
            public readonly float Previous;

            public ChangeData(SyncStopwatchOperation operation, float previous)
            {
                Operation = operation;
                Previous = previous;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Delegate signature for when the Stopwatch operation occurs.
        /// </summary>
        /// <param name = "op">Operation which was performed.</param>
        /// <param name = "prev">Previous value of the Stopwatch. This will be -1f is the value is not available.</param>
        /// <param name = "asServer">True if occurring on server.</param>
        public delegate void SyncTypeChanged(SyncStopwatchOperation op, float prev, bool asServer);

        /// <summary>
        /// Called when a Stopwatch operation occurs.
        /// </summary>
        public event SyncTypeChanged OnChange;
        /// <summary>
        /// How much time has passed since the Stopwatch started.
        /// </summary>
        public float Elapsed { get; private set; } = -1f;
        /// <summary>
        /// True if the SyncStopwatch is currently paused. Calls to Update(float) will be ignored when paused.
        /// </summary>
        public bool Paused { get; private set; }
        #endregion

        #region Private.
        /// <summary>
        /// Changed data which will be sent next tick.
        /// </summary>
        private List<ChangeData> _changed = new();
        /// <summary>
        /// Server OnChange events waiting for start callbacks.
        /// </summary>
        private List<ChangeData> _serverOnChanges = new();
        /// <summary>
        /// Client OnChange events waiting for start callbacks.
        /// </summary>
        private List<ChangeData> _clientOnChanges = new();
        #endregion

        #region Constructors
        public SyncStopwatch(SyncTypeSettings settings = new()) : base(settings) { }
        #endregion

        /// <summary>
        /// Called when the SyncType has been registered, but not yet initialized over the network.
        /// </summary>
        protected override void Initialized()
        {
            base.Initialized();

            // Initialize collections if needed. OdinInspector can cause them to become deinitialized.
#if ODIN_INSPECTOR
            if (_changed == null)
                _changed = new();
            if (_serverOnChanges == null)
                _serverOnChanges = new();
            if (_clientOnChanges == null)
                _clientOnChanges = new();
#endif
        }

        /// <summary>
        /// Starts a Stopwatch. If called when a Stopwatch is already active then StopStopwatch will automatically be sent.
        /// </summary>
        /// <param name = "remaining">Time in which the Stopwatch should start with.</param>
        /// <param name = "sendElapsedOnStop">True to include remaining time when automatically sending StopStopwatch.</param>
        public void StartStopwatch(bool sendElapsedOnStop = true)
        {
            if (!CanNetworkSetValues(true))
                return;

            if (Elapsed > 0f)
                StopStopwatch(sendElapsedOnStop);

            Elapsed = 0f;
            AddOperation(SyncStopwatchOperation.Start, 0f);
        }

        /// <summary>
        /// Pauses the Stopwatch. Calling while already paused will be result in no action.
        /// </summary>
        /// <param name = "sendElapsed">True to send Remaining with this operation.</param>
        public void PauseStopwatch(bool sendElapsed = false)
        {
            if (Elapsed < 0f)
                return;
            if (Paused)
                return;
            if (!CanNetworkSetValues(true))
                return;

            Paused = true;
            float prev;
            SyncStopwatchOperation op;
            if (sendElapsed)
            {
                prev = Elapsed;
                op = SyncStopwatchOperation.PauseUpdated;
            }
            else
            {
                prev = -1f;
                op = SyncStopwatchOperation.Pause;
            }

            AddOperation(op, prev);
        }

        /// <summary>
        /// Unpauses the Stopwatch. Calling while already unpaused will be result in no action.
        /// </summary>
        public void UnpauseStopwatch()
        {
            if (Elapsed < 0f)
                return;
            if (!Paused)
                return;
            if (!CanNetworkSetValues(true))
                return;

            Paused = false;
            AddOperation(SyncStopwatchOperation.Unpause, -1f);
        }

        /// <summary>
        /// Stops and resets the Stopwatch.
        /// </summary>
        public void StopStopwatch(bool sendElapsed = false)
        {
            if (Elapsed < 0f)
                return;
            if (!CanNetworkSetValues(true))
                return;

            float prev = sendElapsed ? -1f : Elapsed;
            StopStopwatch_Internal(true);
            SyncStopwatchOperation op = sendElapsed ? SyncStopwatchOperation.StopUpdated : SyncStopwatchOperation.Stop;
            AddOperation(op, prev);
        }

        /// <summary>
        /// Adds an operation to synchronize.
        /// </summary>
        private void AddOperation(SyncStopwatchOperation operation, float prev)
        {
            if (!IsInitialized)
                return;

            bool asServerInvoke = !IsNetworkInitialized || NetworkBehaviour.IsServerStarted;

            if (asServerInvoke)
            {
                if (Dirty())
                {
                    ChangeData change = new(operation, prev);
                    _changed.Add(change);
                }
            }

            OnChange?.Invoke(operation, prev, asServerInvoke);
        }

        /// <summary>
        /// Writes all changed values.
        /// </summary>
        /// <param name = "resetSyncTick">True to set the next time data may sync.</param>
        protected internal override void WriteDelta(PooledWriter writer, bool resetSyncTick = true)
        {
            base.WriteDelta(writer, resetSyncTick);
            writer.WriteInt32(_changed.Count);

            for (int i = 0; i < _changed.Count; i++)
            {
                ChangeData change = _changed[i];
                writer.WriteUInt8Unpacked((byte)change.Operation);
                if (change.Operation == SyncStopwatchOperation.Start)
                    WriteStartStopwatch(writer, 0f, false);
                // Pause and unpause updated need current value written.
                // Updated stop also writes current value.
                else if (change.Operation == SyncStopwatchOperation.PauseUpdated || change.Operation == SyncStopwatchOperation.StopUpdated)
                    writer.WriteSingle(change.Previous);
            }

            _changed.Clear();
        }

        /// <summary>
        /// Writes all values.
        /// </summary>
        protected internal override void WriteFull(PooledWriter writer)
        {
            // Only write full if a Stopwatch is running.
            if (Elapsed < 0f)
                return;

            base.WriteDelta(writer, false);

            // There will be 1 or 2 entries. If paused 2, if not 1.
            int entries = Paused ? 2 : 1;
            writer.WriteInt32(entries);
            // And the operations.
            WriteStartStopwatch(writer, Elapsed, true);
            if (Paused)
                writer.WriteUInt8Unpacked((byte)SyncStopwatchOperation.Pause);
        }

        /// <summary>
        /// Writers a start with elapsed time.
        /// </summary>
        /// <param name = "elapsed"></param>
        private void WriteStartStopwatch(Writer w, float elapsed, bool includeOperationByte)
        {
            if (includeOperationByte)
                w.WriteUInt8Unpacked((byte)SyncStopwatchOperation.Start);

            w.WriteSingle(elapsed);
        }

        /// <summary>
        /// Reads and sets the current values for server or client.
        /// </summary>
        [APIExclude]
        protected internal override void Read(PooledReader reader, bool asServer)
        {
            SetReadArguments(reader, asServer, out bool newChangeId, out bool asClientHost, out bool canModifyValues);

            int changes = reader.ReadInt32();

            for (int i = 0; i < changes; i++)
            {
                SyncStopwatchOperation op = (SyncStopwatchOperation)reader.ReadUInt8Unpacked();
                if (op == SyncStopwatchOperation.Start)
                {
                    float elapsed = reader.ReadSingle();

                    if (canModifyValues)
                        Elapsed = elapsed;

                    if (newChangeId)
                        InvokeOnChange(op, elapsed, asServer);
                }
                else if (op == SyncStopwatchOperation.Pause)
                {
                    if (canModifyValues)
                        Paused = true;

                    if (newChangeId)
                        InvokeOnChange(op, -1f, asServer);
                }
                else if (op == SyncStopwatchOperation.PauseUpdated)
                {
                    float prev = reader.ReadSingle();

                    if (canModifyValues)
                        Paused = true;

                    if (newChangeId)
                        InvokeOnChange(op, prev, asServer);
                }
                else if (op == SyncStopwatchOperation.Unpause)
                {
                    if (canModifyValues)
                        Paused = false;

                    if (newChangeId)
                        InvokeOnChange(op, -1f, asServer);
                }
                else if (op == SyncStopwatchOperation.Stop)
                {
                    if (canModifyValues)
                        StopStopwatch_Internal(asServer);

                    if (newChangeId)
                        InvokeOnChange(op, -1f, false);
                }
                else if (op == SyncStopwatchOperation.StopUpdated)
                {
                    float prev = reader.ReadSingle();
                    if (canModifyValues)
                        StopStopwatch_Internal(asServer);

                    if (newChangeId)
                        InvokeOnChange(op, prev, asServer);
                }
            }

            if (newChangeId && changes > 0)
                InvokeOnChange(SyncStopwatchOperation.Complete, -1f, asServer);
        }

        /// <summary>
        /// Stops the Stopwatch and resets.
        /// </summary>
        private void StopStopwatch_Internal(bool asServer)
        {
            Paused = false;
            Elapsed = -1f;
        }

        /// <summary>
        /// Invokes OnChanged callback.
        /// </summary>
        private void InvokeOnChange(SyncStopwatchOperation operation, float prev, bool asServer)
        {
            if (asServer)
            {
                if (NetworkBehaviour.OnStartServerCalled)
                    OnChange?.Invoke(operation, prev, asServer);
                else
                    _serverOnChanges.Add(new(operation, prev));
            }
            else
            {
                if (NetworkBehaviour.OnStartClientCalled)
                    OnChange?.Invoke(operation, prev, asServer);
                else
                    _clientOnChanges.Add(new(operation, prev));
            }
        }

        /// <summary>
        /// Called after OnStartXXXX has occurred.
        /// </summary>
        /// <param name = "asServer">True if OnStartServer was called, false if OnStartClient.</param>
        protected internal override void OnStartCallback(bool asServer)
        {
            base.OnStartCallback(asServer);
            List<ChangeData> collection = asServer ? _serverOnChanges : _clientOnChanges;

            if (OnChange != null)
            {
                foreach (ChangeData item in collection)
                    OnChange.Invoke(item.Operation, item.Previous, asServer);
            }

            collection.Clear();
        }

        /// <summary>
        /// Adds delta from Remaining for server and client.
        /// </summary>
        /// <param name = "delta">Value to remove from Remaining.</param>
        public void Update(float delta)
        {
            //Not enabled.
            if (Elapsed == -1f)
                return;
            if (Paused)
                return;

            Elapsed += delta;
        }

        /// <summary>
        /// Return the serialized type.
        /// </summary>
        /// <returns></returns>
        public object GetSerializedType() => null;
    }
}