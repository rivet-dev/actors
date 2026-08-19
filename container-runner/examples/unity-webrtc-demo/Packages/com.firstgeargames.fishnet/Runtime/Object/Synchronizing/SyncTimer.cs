using FishNet.Documenting;
using FishNet.Object.Synchronizing.Internal;
using FishNet.Serializing;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Object.Synchronizing
{
    /// <summary>
    /// A SyncObject to efficiently synchronize timers over the network.
    /// </summary>
    public class SyncTimer : SyncBase, ICustomSync
    {
        #region Type.
        /// <summary>
        /// Information about how the timer has changed.
        /// </summary>
        private struct ChangeData
        {
            public readonly SyncTimerOperation Operation;
            public readonly float Previous;
            public readonly float Next;

            public ChangeData(SyncTimerOperation operation, float previous, float next)
            {
                Operation = operation;
                Previous = previous;
                Next = next;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Delegate signature for when the timer operation occurs.
        /// </summary>
        /// <param name = "op">Operation which was performed.</param>
        /// <param name = "prev">Previous value of the timer. This will be -1f is the value is not available.</param>
        /// <param name = "next">Value of the timer. This will be -1f is the value is not available.</param>
        /// <param name = "asServer">True if occurring on server.</param>
        public delegate void SyncTypeChanged(SyncTimerOperation op, float prev, float next, bool asServer);

        /// <summary>
        /// Called when a timer operation occurs.
        /// </summary>
        public event SyncTypeChanged OnChange;
        /// <summary>
        /// Time remaining on the timer. When the timer is expired this value will be 0f.
        /// </summary>
        public float Remaining { get; private set; }
        /// <summary>
        /// How much time has passed since the timer started.
        /// </summary>
        public float Elapsed => Duration - Remaining;
        /// <summary>
        /// Starting duration of the timer.
        /// </summary>
        public float Duration { get; private set; }
        /// <summary>
        /// True if the SyncTimer is currently paused. Calls to Update(float) will be ignored when paused.
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
        /// <summary>
        /// Last Time.unscaledTime the timer delta was updated.
        /// </summary>
        private float _updateTime;
        #endregion

        #region Constructors
        public SyncTimer(SyncTypeSettings settings = new()) : base(settings) { }
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
        /// Starts a timer. If called when a timer is already active then StopTimer will automatically be sent.
        /// </summary>
        /// <param name = "remaining">Time in which the timer should start with.</param>
        /// <param name = "sendRemainingOnStop">True to include remaining time when automatically sending StopTimer.</param>
        public void StartTimer(float remaining, bool sendRemainingOnStop = true)
        {
            if (!CanNetworkSetValues(true))
                return;

            if (Remaining > 0f)
                StopTimer(sendRemainingOnStop);

            Paused = false;
            Remaining = remaining;
            Duration = remaining;
            SetUpdateTime();
            AddOperation(SyncTimerOperation.Start, -1f, remaining);
        }

        /// <summary>
        /// Pauses the timer. Calling while already paused will be result in no action.
        /// </summary>
        /// <param name = "sendRemaining">True to send Remaining with this operation.</param>
        public void PauseTimer(bool sendRemaining = false)
        {
            if (Remaining <= 0f)
                return;
            if (Paused)
                return;
            if (!CanNetworkSetValues(true))
                return;

            Paused = true;
            SyncTimerOperation op = sendRemaining ? SyncTimerOperation.PauseUpdated : SyncTimerOperation.Pause;
            AddOperation(op, Remaining, Remaining);
        }

        /// <summary>
        /// Unpauses the timer. Calling while already unpaused will be result in no action.
        /// </summary>
        public void UnpauseTimer()
        {
            if (Remaining <= 0f)
                return;
            if (!Paused)
                return;
            if (!CanNetworkSetValues(true))
                return;

            Paused = false;
            SetUpdateTime();
            AddOperation(SyncTimerOperation.Unpause, Remaining, Remaining);
        }

        /// <summary>
        /// Stops and resets the timer.
        /// </summary>
        public void StopTimer(bool sendRemaining = false)
        {
            if (Remaining <= 0f)
                return;
            if (!CanNetworkSetValues(true))
                return;

            bool asServer = true;
            float prev = Remaining;
            StopTimer_Internal(asServer);
            SyncTimerOperation op = sendRemaining ? SyncTimerOperation.StopUpdated : SyncTimerOperation.Stop;
            AddOperation(op, prev, 0f);
        }

        /// <summary>
        /// Adds an operation to synchronize.
        /// </summary>
        private void AddOperation(SyncTimerOperation operation, float prev, float next)
        {
            if (!IsInitialized)
                return;

            bool asServerInvoke = !IsNetworkInitialized || NetworkBehaviour.IsServerStarted;

            if (asServerInvoke)
            {
                if (Dirty())
                {
                    ChangeData change = new(operation, prev, next);
                    _changed.Add(change);
                }
            }

            OnChange?.Invoke(operation, prev, next, asServerInvoke);
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

                if (change.Operation == SyncTimerOperation.Start)
                {
                    WriteStartTimer(writer, false);
                }
                // Pause and unpause updated need current value written.
                // Updated stop also writes current value.
                else if (change.Operation == SyncTimerOperation.PauseUpdated || change.Operation == SyncTimerOperation.StopUpdated)
                {
                    writer.WriteSingle(change.Next);
                }
            }

            _changed.Clear();
        }

        /// <summary>
        /// Writes all values.
        /// </summary>
        protected internal override void WriteFull(PooledWriter writer)
        {
            // Only write full if a timer is running.
            if (Remaining <= 0f)
                return;

            base.WriteDelta(writer, false);
            //There will be 1 or 2 entries. If paused 2, if not 1.
            int entries = Paused ? 2 : 1;
            writer.WriteInt32(entries);
            //And the operations.
            WriteStartTimer(writer, true);
            if (Paused)
                writer.WriteUInt8Unpacked((byte)SyncTimerOperation.Pause);
        }

        /// <summary>
        /// Writes a StartTimer operation.
        /// </summary>
        /// <param name = "w"></param>
        /// <param name = "includeOperationByte"></param>
        private void WriteStartTimer(Writer w, bool includeOperationByte)
        {
            if (includeOperationByte)
                w.WriteUInt8Unpacked((byte)SyncTimerOperation.Start);
            w.WriteSingle(Remaining);
            w.WriteSingle(Duration);
        }

        /// <summary>
        /// Reads and sets the current values for server or client.
        /// </summary>
        [APIExclude]
        protected internal override void Read(PooledReader reader, bool asServer)
        {
            SetReadArguments(reader, asServer, out bool newChangeId, out bool asClientHost, out bool canModifyValues);

            int changes = reader.ReadInt32();
            //Has previous value if should invoke finished.
            float? finishedPrevious = null;

            for (int i = 0; i < changes; i++)
            {
                SyncTimerOperation op = (SyncTimerOperation)reader.ReadUInt8Unpacked();
                if (op == SyncTimerOperation.Start)
                {
                    float next = reader.ReadSingle();
                    float duration = reader.ReadSingle();

                    if (canModifyValues)
                    {
                        SetUpdateTime();
                        Paused = false;
                        Remaining = next;
                        Duration = duration;
                    }

                    if (newChangeId)
                    {
                        InvokeOnChange(op, -1f, next, asServer);
                        /* If next is 0 then that means the timer
                         * expired on the same tick it was started.
                         * This can be true depending on when in code
                         * the server starts the timer.
                         *
                         * When 0 also invoke finished. */
                        if (next == 0)
                            finishedPrevious = duration;
                    }
                }
                else if (op == SyncTimerOperation.Pause || op == SyncTimerOperation.PauseUpdated || op == SyncTimerOperation.Unpause)
                {
                    if (canModifyValues)
                        UpdatePauseState(op);
                }
                else if (op == SyncTimerOperation.Stop)
                {
                    float prev = Remaining;

                    if (canModifyValues)
                        StopTimer_Internal(asServer);

                    if (newChangeId)
                        InvokeOnChange(op, prev, 0f, false);
                }
                //
                else if (op == SyncTimerOperation.StopUpdated)
                {
                    float prev = Remaining;
                    float next = reader.ReadSingle();

                    if (canModifyValues)
                        StopTimer_Internal(asServer);

                    if (newChangeId)
                        InvokeOnChange(op, prev, next, asServer);
                }
            }

            //Updates a pause state with a pause or unpause operation.
            void UpdatePauseState(SyncTimerOperation op)
            {
                bool newPauseState = op == SyncTimerOperation.Pause || op == SyncTimerOperation.PauseUpdated;

                float prev = Remaining;
                float next;
                //If updated time as well.
                if (op == SyncTimerOperation.PauseUpdated)
                {
                    next = reader.ReadSingle();
                    Remaining = next;
                }
                else
                {
                    next = Remaining;
                }

                Paused = newPauseState;
                if (!Paused)
                    SetUpdateTime();
                if (newChangeId)
                    InvokeOnChange(op, prev, next, asServer);
            }

            if (newChangeId && changes > 0)
                InvokeOnChange(SyncTimerOperation.Complete, -1f, -1f, false);
            if (finishedPrevious.HasValue)
                InvokeFinished(finishedPrevious.Value);
        }

        /// <summary>
        /// Stops the timer and resets.
        /// </summary>
        private void StopTimer_Internal(bool asServer)
        {
            Paused = false;
            Remaining = 0f;
        }

        /// <summary>
        /// Invokes OnChanged callback.
        /// </summary>
        private void InvokeOnChange(SyncTimerOperation operation, float prev, float next, bool asServer)
        {
            if (asServer)
            {
                if (NetworkBehaviour.OnStartServerCalled)
                    OnChange?.Invoke(operation, prev, next, asServer);
                else
                    _serverOnChanges.Add(new(operation, prev, next));
            }
            else
            {
                if (NetworkBehaviour.OnStartClientCalled)
                    OnChange?.Invoke(operation, prev, next, asServer);
                else
                    _clientOnChanges.Add(new(operation, prev, next));
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
                    OnChange.Invoke(item.Operation, item.Previous, item.Next, asServer);
            }

            collection.Clear();
        }

        /// <summary>
        /// Sets updateTime to current values.
        /// </summary>
        private void SetUpdateTime()
        {
            _updateTime = Time.unscaledTime;
        }

        /// <summary>
        /// Removes time passed from Remaining since the last unscaled time using this method.
        /// </summary>
        public void Update()
        {
            float delta = Time.unscaledTime - _updateTime;
            Update(delta);
        }

        /// <summary>
        /// Removes delta from Remaining for server and client.
        /// This also resets unscaledTime delta for Update().
        /// </summary>
        /// <param name = "delta">Value to remove from Remaining.</param>
        public void Update(float delta)
        {
            //Not enabled.
            if (Remaining <= 0f)
                return;
            if (Paused)
                return;

            SetUpdateTime();
            if (delta < 0)
                delta *= -1f;
            float prev = Remaining;
            Remaining -= delta;
            //Still time left.
            if (Remaining > 0f)
                return;

            /* If here then the timer has
             * ended. Invoking the events is tricky
             * here because both the server and the client
             * would share the same value. Because of this check
             * if each socket is started and if so invoke for that
             * side. There's a chance down the road this may need to be improved
             * for some but at this time I'm unable to think of any
             * problems. */
            Remaining = 0f;
            InvokeFinished(prev);
        }

        /// <summary>
        /// Invokes SyncTimer finished a previous value.
        /// </summary>
        /// <param name = "prev"></param>
        private void InvokeFinished(float prev)
        {
            if (NetworkManager.IsServerStarted)
                OnChange?.Invoke(SyncTimerOperation.Finished, prev, 0f, true);
            if (NetworkManager.IsClientStarted)
                OnChange?.Invoke(SyncTimerOperation.Finished, prev, 0f, false);
        }

        /// <summary>
        /// Return the serialized type.
        /// </summary>
        /// <returns></returns>
        public object GetSerializedType() => null;
    }
}