using FishNet.CodeGenerating;
using FishNet.Documenting;
using FishNet.Managing;
using FishNet.Object.Helping;
using FishNet.Object.Synchronizing.Internal;
using FishNet.Serializing;
using FishNet.Serializing.Helping;
using System.Runtime.InteropServices;
using UnityEngine;

namespace FishNet.Object.Synchronizing
{
    internal interface ISyncVar { }

    [APIExclude]
    [System.Serializable]
    [StructLayout(LayoutKind.Auto, CharSet = CharSet.Auto)]
    public class SyncVar<T> : SyncBase, ISyncVar
    {
        #region Types.
        public struct InterpolationContainer
        {
            /// <summary>
            /// Value prior to setting new.
            /// </summary>
            public T LastValue;
            /// <summary>
            /// Tick when LastValue was set.
            /// </summary>
            public float UpdateTime;

            public void Update(T prevValue)
            {
                LastValue = prevValue;
                UpdateTime = Time.unscaledTime;
            }
        }

        /// <summary>
        /// Information needed to invoke a callback.
        /// </summary>
        private struct CachedOnChange
        {
            internal readonly T Previous;
            internal readonly T Next;

            public CachedOnChange(T previous, T next)
            {
                Previous = previous;
                Next = next;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Value interpolated between last received and current.
        /// </summary>
        /// <param name = "useCurrentValue">
        /// True if to ignore interpolated calculations and use the current value.
        /// This can be useful if you are able to write this SyncVars values in update.
        /// </param>
        public T InterpolatedValue(bool useCurrentValue = false)
        {
            if (useCurrentValue)
                return _value;

            float diff = Time.unscaledTime - _interpolator.UpdateTime;
            float percent = Mathf.InverseLerp(0f, Settings.SendRate, diff);

            return Interpolate(_interpolator.LastValue, _value, percent);
        }

        /// <summary>
        /// Gets and sets the current value for this SyncVar.
        /// </summary>
        public T Value
        {
            get => _value;
            set => SetValue(value, true);
        }
        ///// <summary>
        ///// Sets the current value for this SyncVar while sending it immediately.
        ///// </summary>
        // public T ValueRpc
        // {
        //    set => SetValue(value, true, true);
        // }
        ///// <summary>
        ///// Gets the current value for this SyncVar while marking it dirty. This could be useful to change properties or fields on a reference type SyncVar and have the SyncVar be dirtied after.
        ///// </summary>
        // public T ValueDirty
        // {
        //    get
        //    {
        //        base.Dirty();
        //        return _value;
        //    }
        // }
        ///// <summary>
        ///// Gets the current value for this SyncVar while sending it imediately. This could be useful to change properties or fields on a reference type SyncVar and have the SyncVar send after.
        ///// </summary>
        // public T ValueDirtyRpc
        // {
        //    get
        //    {
        //        base.Dirty(true);
        //        return _value;
        //    }
        // }
        /// <summary>
        /// Called when the SyncDictionary changes.
        /// </summary>
        public event OnChanged OnChange;

        public delegate void OnChanged(T prev, T next, bool asServer);
        #endregion

        #region Private.
        /// <summary>
        /// Server OnChange event waiting for start callbacks.
        /// </summary>
        private CachedOnChange? _serverOnChange;
        /// <summary>
        /// Client OnChange event waiting for start callbacks.
        /// </summary>
        private CachedOnChange? _clientOnChange;
        /// <summary>
        /// Value before the network is initialized on the containing object.
        /// </summary>
        private T _initialValue;
        /// <summary>
        /// Current value on the server, or client.
        /// </summary>
        [SerializeField]
        private T _value;
        /// <summary>
        /// Holds information about interpolating between values.
        /// </summary>
        private InterpolationContainer _interpolator = new();
        /// <summary>
        /// True if value was ever set after the SyncType initialized.
        /// This is true even if SetInitialValues was called at runtime.
        /// </summary>
        private bool _valueSetAfterInitialized;
        #endregion

        #region Constructors.
        public SyncVar(SyncTypeSettings settings = new()) : this(default, settings) { }
        public SyncVar(T initialValue, SyncTypeSettings settings = new()) : base(settings) => SetInitialValues(initialValue);
        #endregion

        /// <summary>
        /// Called when the SyncType has been registered, but not yet initialized over the network.
        /// </summary>
        protected override void Initialized()
        {
            base.Initialized();
            _initialValue = _value;
        }

        /// <summary>
        /// Sets initial values.
        /// Initial values are not automatically synchronized, as it is assumed clients and server already have them set to the specified value.
        /// When a SyncVar is reset, such as when the object despawns, current values are set to initial values.
        /// </summary>
        public void SetInitialValues(T value)
        {
            _initialValue = value;
            /* Only update current if a value has not been set already.
             * A value normally would not be set unless a SyncVar came through
             * as the object was enabling, such as if it started in a disabled state
             * and was later enabled. */
            if (!_valueSetAfterInitialized)
                UpdateValues(value);

            if (IsInitialized)
                _valueSetAfterInitialized = true;
        }

        /// <summary>
        /// Sets current and previous values.
        /// </summary>
        /// <param name = "next"></param>
        private void UpdateValues(T next)
        {
            // If network initialized then update interpolator.
            if (IsNetworkInitialized)
                _interpolator.Update(_value);

            _value = next;
        }

        /// <summary>
        /// Sets current value and marks the SyncVar dirty when able to. Returns if able to set value.
        /// </summary>
        /// <param name = "calledByUser">True if SetValue was called in response to user code. False if from automated code.</param>
        internal void SetValue(T nextValue, bool calledByUser, bool sendRpc = false)
        {
            /* IsInitialized is only set after the script containing this SyncVar
             * has executed our codegen in the beginning of awake, and after awake
             * user logic. When not set update the initial values */
            if (!IsInitialized)
            {
                SetInitialValues(nextValue);
                return;
            }

            _valueSetAfterInitialized = true;

            /* If not client or server then set skipChecks
             * as true. When neither is true it's likely user is changing
             * value before object is initialized. This is allowed
             * but checks cannot be processed because they would otherwise
             * stop setting the value. */
            bool isNetworkInitialized = IsNetworkInitialized;

            // Object is deinitializing.
            if (isNetworkInitialized && CodegenHelper.NetworkObject_Deinitializing(NetworkBehaviour))
                return;

            // If being set by user code.
            if (calledByUser)
            {
                if (!CanNetworkSetValues(log: true))
                    return;
                /* We will only be this far if the network is not active yet,
                 * server is active, or client has setting permissions.
                 * We only need to set asServerInvoke to false if the network
                 * is initialized and the server is not active. */
                bool asServerInvoke = CanInvokeCallbackAsServer();

                /* If the network has not been network initialized then
                 * Value is expected to be set on server and client since
                 * it's being set before the object is initialized. */
                if (!isNetworkInitialized)
                {
                    T prev = _value;
                    UpdateValues(nextValue);
                    // Still call invoke because change will be cached for when the network initializes.
                    InvokeOnChange(prev, _value, asServer: true);
                }
                else
                {
                    if (Comparers.EqualityCompare(_value, nextValue))
                        return;

                    T prev = _value;
                    UpdateValues(nextValue);
                    InvokeOnChange(prev, _value, asServerInvoke);
                }

                TryDirty(asServerInvoke);
            }
            // Not called by user.
            else
            {
                /* Only perform the equality checks when not host.
                 *
                 * In the previous SyncVar version it was okay to call
                 * this on host because a separate clientHost value was kept for
                 * the client side, and that was compared against.
                 *
                 * In newer SyncVar(this one) a client side copy is
                 * not kept so when compariing against the current vlaue
                 * as clientHost, it will always return as matched.
                 *
                 * But it's impossible for clientHost to send a value
                 * and it not have changed, so this check is not needed. */

                // /* Previously clients were not allowed to set values
                //  * but this has been changed because clients may want
                //  * to update values locally while occasionally
                //  * letting the syncvar adjust their side. */
                // T prev = _value;
                // if (Comparers.EqualityCompare(prev, nextValue))
                //     return;

                T prev = _value;
                /* If also server do not update value.
                 * Server side has say of the current value. */
                /* Only update value if not server. We do not want
                 * clientHost overwriting servers current with what
                 * they just received.*/
                if (!NetworkManager.IsServerStarted)
                    UpdateValues(nextValue);

                InvokeOnChange(prev, nextValue, asServer: false);
            }


            /* Tries to dirty so update
             * is sent over network. This needs to be called
             * anytime the data changes because there is no way
             * to know if the user set the value on both server
             * and client or just one side. */
            void TryDirty(bool asServer)
            {
                //Cannot dirty when network is not initialized.
                if (!isNetworkInitialized)
                    return;

                if (asServer)
                    Dirty();
                //base.Dirty(sendRpc);
            }
        }

        /// <summary>
        /// Returns interpolated values between previous and current using a percentage.
        /// </summary>
        protected virtual T Interpolate(T previous, T current, float percent)
        {
            NetworkManager.LogError($"Type {typeof(T).FullName} does not support interpolation. Implement a supported type class or create your own. See class FloatSyncVar for an example.");
            return default;
        }

        /// <summary>
        /// True if callback can be invoked with asServer true.
        /// </summary>
        /// <returns></returns>
        private bool AsServerInvoke() => !IsNetworkInitialized || NetworkBehaviour.IsServerStarted;

        /// <summary>
        /// Dirties the the syncVar for a full send.
        /// </summary>
        public void DirtyAll()
        {
            if (!IsInitialized)
                return;
            if (!CanNetworkSetValues(log: true))
                return;

            //Also set that values have changed since the user is forcing a sync.
            _valueSetAfterInitialized = true;

            Dirty();
            /* Invoke even if was unable to dirty. Dirtying only
             * becomes true if server is running, but also if there are
             * observers. Even if there are not observers we still want
             * to invoke for the server side. */
            //todo: this behaviour needs to be done for all synctypes with dirt/dirtyall.
            bool asServerInvoke = CanInvokeCallbackAsServer();
            InvokeOnChange(_value, _value, asServerInvoke);
        }

        /// <summary>
        /// Invokes OnChanged callback.
        /// </summary>
        private void InvokeOnChange(T prev, T next, bool asServer)
        {
            if (asServer)
            {
                if (NetworkBehaviour.OnStartServerCalled)
                    OnChange?.Invoke(prev, next, asServer);
                else
                    _serverOnChange = new CachedOnChange(prev, next);
            }
            else
            {
                if (NetworkBehaviour.OnStartClientCalled)
                    OnChange?.Invoke(prev, next, asServer);
                else
                    _clientOnChange = new CachedOnChange(prev, next);
            }
        }

        /// <summary>
        /// Called after OnStartXXXX has occurred.
        /// </summary>
        /// <param name = "asServer">True if OnStartServer was called, false if OnStartClient.</param>
        [MakePublic]
        protected internal override void OnStartCallback(bool asServer)
        {
            base.OnStartCallback(asServer);

            if (OnChange != null)
            {
                CachedOnChange? change = asServer ? _serverOnChange : _clientOnChange;
                if (change != null)
                    InvokeOnChange(change.Value.Previous, change.Value.Next, asServer);
            }

            if (asServer)
                _serverOnChange = null;
            else
                _clientOnChange = null;
        }

        /// <summary>
        /// Writes current value.
        /// </summary>
        /// <param name = "resetSyncTick">True to set the next time data may sync.</param>
        [MakePublic]
        protected internal override void WriteDelta(PooledWriter writer, bool resetSyncTick = true)
        {
            base.WriteDelta(writer, resetSyncTick);
            writer.Write(_value);
        }

        /// <summary>
        /// Writes current value if not initialized value.
        /// </summary>
        /// m>
        [MakePublic]
        protected internal override void WriteFull(PooledWriter obj0)
        {
            // /* If a class then skip comparer check.
            //  * InitialValue and Value will be the same reference.
            //  *
            //  * If a value then compare field changes, since the references
            //  * will not be the same. */
            // //Compare if a value type.
            // if (_isValueType)
            // {
            //     if (Comparers.EqualityCompare(_initialValue, _value))
            //         return;
            // }
            // else
            // {
            //     if (!_valueSetAfterInitialized)
            //         return;
            // }

            if (!_valueSetAfterInitialized)
                return;

            /* SyncVars only hold latest value, so just
             * write current delta. */
            WriteDelta(obj0, false);
        }

        /// <summary>
        /// Reads a SyncVar value.
        /// </summary>
        protected internal override void Read(PooledReader reader, bool asServer)
        {
            T value = reader.Read<T>();

            if (!ReadChangeId(reader))
                return;

            SetValue(value, false);
            //TODO this needs to separate invokes from setting values so that syncvar can be written like remainder of synctypes.
        }

        //SyncVars do not use changeId.
        [APIExclude]
        protected override bool ReadChangeId(Reader reader) => true;

        //SyncVars do not use changeId.
        [APIExclude]
        protected override void WriteChangeId(PooledWriter writer) { }

        /// <summary>
        /// Resets to initialized values.
        /// </summary>
        [MakePublic]
        protected internal override void ResetState(bool asServer)
        {
            base.ResetState(asServer);
            /* Only full reset under the following conditions:
             * asServer is true.
             * Is not network initialized.
             * asServer is false, and server is not started. */
            if (CanReset(asServer))
            {
                _value = _initialValue;
                _valueSetAfterInitialized = false;
            }
        }
    }
}