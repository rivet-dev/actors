#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Component.Transforming;
using FishNet.Connection;
using FishNet.Documenting;
using FishNet.Managing.Logging;
using FishNet.Managing.Server;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Utility;
using FishNet.Utility.Performance;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using FishNet.Managing;
using Unity.Profiling;
using UnityEngine;
using TimeManagerCls = FishNet.Managing.Timing.TimeManager;

namespace FishNet.Component.Animating
{
    [AddComponentMenu("FishNet/Component/NetworkAnimator")]
    public sealed class NetworkAnimator : NetworkBehaviour
    {
        #region Types.
        /// <summary>
        /// Data received from the server.
        /// </summary>
        private struct ReceivedServerData
        {
            /// <summary>
            /// Gets an Arraysegment of received data.
            /// </summary>
            public ArraySegment<byte> GetArraySegment() => new(_data, 0, _length);

            /// <summary>
            /// How much data written.
            /// </summary>
            private int _length;
            /// <summary>
            /// Buffer which contains data.
            /// </summary>
            private byte[] _data;

            public ReceivedServerData(ArraySegment<byte> segment)
            {
                _length = segment.Count;
                _data = ByteArrayPool.Retrieve(_length);
                Buffer.BlockCopy(segment.Array, segment.Offset, _data, 0, _length);
            }

            public void Dispose()
            {
                if (_data != null)
                    ByteArrayPool.Store(_data);
            }
        }

        private struct StateChange
        {
            /// <summary>
            /// Frame which the state was changed.
            /// </summary>
            public int FrameCount;
            /// <summary>
            /// True if a crossfade.
            /// </summary>
            public bool IsCrossfade;
            /// <summary>
            /// Hash to crossfade into.
            /// </summary>
            public int Hash;
            /// <summary>
            /// True if using FixedTime.
            /// </summary>
            public bool FixedTime;
            /// <summary>
            /// Duration of crossfade.
            /// </summary>
            public float DurationTime;
            /// <summary>
            /// Offset time of crossfade.
            /// </summary>
            public float OffsetTime;
            /// <summary>
            /// Normalized transition time of crossfade.
            /// </summary>
            public float NormalizedTransitionTime;

            public StateChange(int frame)
            {
                FrameCount = frame;
                IsCrossfade = default;
                Hash = default;
                FixedTime = default;
                DurationTime = default;
                OffsetTime = default;
                NormalizedTransitionTime = default;
            }

            public StateChange(int frame, int hash, bool fixedTime, float duration, float offset, float normalizedTransition)
            {
                FrameCount = frame;
                IsCrossfade = true;
                Hash = hash;
                FixedTime = fixedTime;
                DurationTime = duration;
                OffsetTime = offset;
                NormalizedTransitionTime = normalizedTransition;
            }
        }

        /// <summary>
        /// Animator updates received from clients when using Client Authoritative.
        /// </summary>
        private class ClientAuthoritativeUpdate
        {
            /// <summary>
            /// </summary>
            public ClientAuthoritativeUpdate()
            {
                // Start buffers off at 8 bytes nad grow them as needed.
                for (int i = 0; i < MAXIMUM_BUFFER_COUNT; i++)
                    _buffers.Add(new byte[MAXIMUM_DATA_SIZE]);

                _bufferLengths = new int[MAXIMUM_BUFFER_COUNT];
            }

            #region Public.
            /// <summary>
            /// True to force all animator data and ignore buffers.
            /// </summary>
            public bool ForceAll { get; private set; }
            /// <summary>
            /// Number of entries in Buffers.
            /// </summary>
            public int BufferCount = 0;
            #endregion

            #region Private.
            /// <summary>
            /// Length of buffers.
            /// </summary>
            private int[] _bufferLengths;
            /// <summary>
            /// Buffers.
            /// </summary>
            private List<byte[]> _buffers = new();
            #endregion

            #region Const.
            /// <summary>
            /// Maximum size data may be.
            /// </summary>
            private const int MAXIMUM_DATA_SIZE = 1000;
            /// <summary>
            /// Maximum number of allowed buffers.
            /// </summary>
            public const int MAXIMUM_BUFFER_COUNT = 2;
            #endregion

            public void AddToBuffer(ref ArraySegment<byte> data)
            {
                int dataCount = data.Count;
                /* Data will never get this large, it's quite impossible.
                 * Just ignore the data if it does, client is likely performing
                 * an attack. */
                if (dataCount > MAXIMUM_DATA_SIZE)
                    return;

                // If index exceeds buffer count.
                if (BufferCount >= MAXIMUM_BUFFER_COUNT)
                {
                    ForceAll = true;
                    return;
                }

                /* If here, can write to buffer. */
                byte[] buffer = _buffers[BufferCount];
                Buffer.BlockCopy(data.Array, data.Offset, buffer, 0, dataCount);
                _bufferLengths[BufferCount] = dataCount;
                BufferCount++;
            }

            /// <summary>
            /// Sets referenced data to buffer and it's length for index.
            /// </summary>
            /// <param name = "index"></param>
            /// <param name = "buffer"></param>
            /// <param name = "length"></param>
            public void GetBuffer(int index, ref byte[] buffer, ref int length)
            {
                if (index > _buffers.Count)
                {
                    NetworkManagerExtensions.LogWarning("Index exceeds Buffers count.");
                    return;
                }
                if (index > _bufferLengths.Length)
                {
                    NetworkManagerExtensions.LogWarning("Index exceeds BufferLengths count.");
                    return;
                }

                buffer = _buffers[index];
                length = _bufferLengths[index];
            }

            /// <summary>
            /// Resets buffers.
            /// </summary>
            public void Reset()
            {
                BufferCount = 0;
                ForceAll = false;
            }
        }

        /// <summary>
        /// Information on how to smooth to a float value.
        /// </summary>
        private struct SmoothedFloat
        {
            public SmoothedFloat(float rate, float target)
            {
                Rate = rate;
                Target = target;
            }

            public readonly float Rate;
            public readonly float Target;
        }

        /// <summary>
        /// Details about a trigger update.
        /// </summary>
        private struct TriggerUpdate
        {
            public byte ParameterIndex;
            public bool Setting;

            public TriggerUpdate(byte parameterIndex, bool setting)
            {
                ParameterIndex = parameterIndex;
                Setting = setting;
            }
        }

        /// <summary>
        /// Details about an animator parameter.
        /// </summary>
        private class ParameterDetail
        {
            /// <summary>
            /// Parameter information.
            /// </summary>
            public readonly AnimatorControllerParameter ControllerParameter = null;
            /// <summary>
            /// Index within the types collection for this parameters value. The exception is with triggers; if the parameter type is a trigger then a value of 1 is set, 0 is unset.
            /// </summary>
            public readonly byte TypeIndex = 0;
            /// <summary>
            /// Hash for the animator string.
            /// </summary>
            public readonly int Hash;

            public ParameterDetail(AnimatorControllerParameter controllerParameter, byte typeIndex)
            {
                ControllerParameter = controllerParameter;
                TypeIndex = typeIndex;
                Hash = controllerParameter.nameHash;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Parameters which will not be synchronized.
        /// </summary>
        [SerializeField]
        [HideInInspector]
        internal List<string> IgnoredParameters = new();
        #endregion

        #region Serialized.
        /// <summary>
        /// The animator component to synchronize.
        /// </summary>
        [Tooltip("The animator component to synchronize.")]
        [SerializeField]
        private Animator _animator;
        /// <summary>
        /// The animator component to synchronize.
        /// </summary>
        public Animator Animator
        {
            get { return _animator; }
        }
        /// <summary>
        /// True to synchronize changes even when the animator component is disabled.
        /// </summary>
        [Tooltip("True to synchronize changes even when the animator component is disabled.")]
        [SerializeField]
        private bool _synchronizeWhenDisabled;
        /// <summary>
        /// True to smooth float value changes for spectators.
        /// </summary>
        [Tooltip("True to smooth float value changes for spectators.")]
        [SerializeField]
        private bool _smoothFloats = true;
        /// <summary>
        /// How many ticks to interpolate.
        /// </summary>
        [Tooltip("How many ticks to interpolate.")]
        [Range(1, NetworkTransform.MAX_INTERPOLATION)]
        [SerializeField]
        private ushort _interpolation = 2;
        /// <summary>
        /// </summary>
        [Tooltip("True if using client authoritative animations.")]
        [SerializeField]
        private bool _clientAuthoritative = true;
        /// <summary>
        /// True if using client authoritative animations.
        /// </summary>
        public bool ClientAuthoritative
        {
            get { return _clientAuthoritative; }
        }
        /// <summary>
        /// True to synchronize server results back to owner. Typically used when you are changing animations on the server and are relying on the server response to update the clients animations.
        /// </summary>
        [Tooltip("True to synchronize server results back to owner. Typically used when you are changing animations on the server and are relying on the server response to update the clients animations.")]
        [SerializeField]
        private bool _sendToOwner;
        #endregion

        #region Private.
        /// <summary>
        /// All parameter values, excluding triggers.
        /// </summary>
        private readonly List<ParameterDetail> _parameterDetails = new();
        /// <summary>
        /// Last int values.
        /// </summary>
        private readonly List<int> _ints = new();
        /// <summary>
        /// Last float values.
        /// </summary>
        private readonly List<float> _floats = new();
        /// <summary>
        /// Last bool values.
        /// </summary>
        private readonly List<bool> _bools = new();
        /// <summary>
        /// Last layer weights.
        /// </summary>
        private float[] _layerWeights;
        /// <summary>
        /// Last speed.
        /// </summary>
        private float _speed;
        /// <summary>
        /// Trigger values set by using SetTrigger and ResetTrigger.
        /// </summary>
        private readonly List<TriggerUpdate> _triggerUpdates = new();
        // /// <summary>
        // /// Updates going to clients.
        // /// </summary>
        // private List<byte[]> _toClientsBuffer = new();
        /// <summary>
        /// Returns if the animator is exist and can be synchronized.
        /// </summary>
        private bool _canSynchronizeAnimator
        {
            get
            {
                if (!_isAnimatorSet)
                    return false;

                if (_animator.enabled || _synchronizeWhenDisabled)
                    return true;

                return false;
            }
        }
        /// <summary>
        /// True if the animator is valid but not enabled.
        /// </summary>
        private bool _isAnimatorSet
        {
            get
            {
                bool failedChecks = _animator == null || _animator.runtimeAnimatorController == null;
                return !failedChecks;
            }
        }
        /// <summary>
        /// Float valeus to smooth towards.
        /// </summary>
        private Dictionary<int, SmoothedFloat> _smoothedFloats = new();
        /// <summary>
        /// Returns if floats can be smoothed for this client.
        /// </summary>
        private bool _canSmoothFloats
        {
            get
            {
                // Don't smooth on server only.
                if (!IsClientStarted)
                    return false;
                // Smoothing is disabled.
                if (!_smoothFloats)
                    return false;
                // No reason to smooth for self.
                if (IsOwner && ClientAuthoritative)
                    return false;

                //Fall through.
                return true;
            }
        }
        /// <summary>
        /// Layers which need to have their state synchronized. Key is the layer, Value is the state change information.
        /// </summary>
        private Dictionary<int, StateChange> _unsynchronizedLayerStates = new();
        /// <summary>
        /// Last animator set.
        /// </summary>
        private Animator _lastAnimator;
        /// <summary>
        /// Last Controller set.
        /// </summary>
        private RuntimeAnimatorController _lastController;
        /// <summary>
        /// PooledWriter for this animator.
        /// </summary>
        private PooledWriter _writer = new();
        /// <summary>
        /// Holds client authoritative updates received to send to other clients.
        /// </summary>
        private ClientAuthoritativeUpdate _clientAuthoritativeUpdates;
        /// <summary>
        /// True to forceAll next timed send.
        /// </summary>
        private bool _forceAllOnTimed;
        /// <summary>
        /// Animations received which should be applied.
        /// </summary>
        private Queue<ReceivedServerData> _fromServerBuffer = new();
        /// <summary>
        /// Tick when the buffer may begin to run.
        /// </summary>
        private uint _startTick = TimeManagerCls.UNSET_TICK;
        /// <summary>
        /// True if subscribed to TimeManager for ticks.
        /// </summary>
        private bool _subscribedToTicks;
        #endregion

        #region Private Profiler Markers
        private static readonly ProfilerMarker _pm_OnPreTick = new("NetworkAnimator.TimeManager_OnPreTick()");
        private static readonly ProfilerMarker _pm_OnPostTick = new("NetworkAnimator.TimeManager_OnPostTick()");
        private static readonly ProfilerMarker _pm_OnUpdate = new("NetworkAnimator.TimeManager_OnUpdate()");
        private static readonly ProfilerMarker _pm_CheckSendToServer = new("NetworkAnimator.CheckSendToServer()");
        private static readonly ProfilerMarker _pm_CheckSendToClients = new("NetworkAnimator.CheckSendToClients()");
        private static readonly ProfilerMarker _pm_SmoothFloats = new("NetworkAnimator.SmoothFloats()");
        private static readonly ProfilerMarker _pm_AnimatorUpdated = new("NetworkAnimator.AnimatorUpdated(ref ArraySegment<byte>, bool)");
        private static readonly ProfilerMarker _pm_ApplyParametersUpdated = new("NetworkAnimator.ApplyParametersUpdated(ref ArraySegment<byte>)");
        #endregion

        #region Const.
        ///// <summary>
        ///// How much time to fall behind when using smoothing. Only increase value if the smoothing is sometimes jittery. Recommended values are between 0 and 0.04.
        ///// </summary>
        //private const float INTERPOLATION = 0.02f;
        /// <summary>
        /// ParameterDetails index which indicates a layer weight change.
        /// </summary>
        private const byte LAYER_WEIGHT = 240;
        /// <summary>
        /// ParameterDetails index which indicates an animator speed change.
        /// </summary>
        private const byte SPEED = 241;
        /// <summary>
        /// ParameterDetails index which indicates a layer state change.
        /// </summary>
        private const byte STATE = 242;
        /// <summary>
        /// ParameterDetails index which indicates a crossfade change.
        /// </summary>
        private const byte CROSSFADE = 243;
        #endregion

        private void Awake()
        {
            InitializeOnce();
        }

        private void OnDestroy()
        {
            ChangeTickSubscription(false);
        }

        [APIExclude]
        public override void OnSpawnServer(NetworkConnection connection)
        {
            if (!_canSynchronizeAnimator)
                return;
            if (AnimatorUpdated(out ArraySegment<byte> updatedBytes, true))
                TargetAnimatorUpdated(connection, updatedBytes);
        }

        public override void OnStartNetwork()
        {
            ChangeTickSubscription(true);
        }

        [APIExclude]
        public override void OnStartServer()
        {
            //If using client authoritative then initialize clientAuthoritativeUpdates.
            if (_clientAuthoritative)
            {
                _clientAuthoritativeUpdates = new();
                // //Expand to clients buffer count to however many buffers can be held.
                // for (int i = 0; i < ClientAuthoritativeUpdate.MAXIMUM_BUFFER_COUNT; i++)
                //     _toClientsBuffer.Add(new byte[0]);
            }
            // else
            // {
            //     _toClientsBuffer.Add(new byte[0]);
            // }
        }

        public override void OnStartClient()
        {
            TimeManager.OnUpdate += TimeManager_OnUpdate;
        }

        public override void OnStopClient()
        {
            if (TimeManager != null)
                TimeManager.OnUpdate -= TimeManager_OnUpdate;
        }

        public override void OnStopNetwork()
        {
            _unsynchronizedLayerStates.Clear();
            ChangeTickSubscription(false);
        }

        /// <summary>
        /// Tries to subscribe to TimeManager ticks.
        /// </summary>
        private void ChangeTickSubscription(bool subscribe)
        {
            if (subscribe == _subscribedToTicks || NetworkManager == null)
                return;

            _subscribedToTicks = subscribe;
            if (subscribe)
            {
                NetworkManager.TimeManager.OnPreTick += TimeManager_OnPreTick;
                NetworkManager.TimeManager.OnPostTick += TimeManager_OnPostTick;
            }
            else
            {
                NetworkManager.TimeManager.OnPreTick -= TimeManager_OnPreTick;
                NetworkManager.TimeManager.OnPostTick -= TimeManager_OnPostTick;
            }
        }

        /// <summary>
        /// Called right before a tick occurs, as well before data is read.
        /// </summary>
        private void TimeManager_OnPreTick()
        {
            using (_pm_OnPreTick.Auto())
            {
                if (!_canSynchronizeAnimator)
                {
                    _fromServerBuffer.Clear();
                    return;
                }
                //Disabled/cannot start.
                if (_startTick == 0)
                    return;
                //Nothing in queue.
                if (_fromServerBuffer.Count == 0)
                {
                    _startTick = 0;
                    return;
                }
                //Not enough time has passed to start queue.
                if (TimeManager.LocalTick < _startTick)
                    return;

                ReceivedServerData rd = _fromServerBuffer.Dequeue();
                ArraySegment<byte> segment = rd.GetArraySegment();
                ApplyParametersUpdated(ref segment);
                rd.Dispose();
            }
        }

        /* Use post tick values are checked after
         * client has an opportunity to use OnTick. */
        /// <summary>
        /// Called after a tick occurs; physics would have simulated if using PhysicsMode.TimeManager.
        /// </summary>
        private void TimeManager_OnPostTick()
        {
            using (_pm_OnPostTick.Auto())
            {
                //One check rather than per each method.
                if (!_canSynchronizeAnimator)
                    return;

                CheckSendToServer();
                CheckSendToClients();
            }
        }

        private void TimeManager_OnUpdate()
        {
            using (_pm_OnUpdate.Auto())
            {
                if (!_canSynchronizeAnimator)
                    return;

                if (IsClientStarted)
                    SmoothFloats();
            }
        }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        private void InitializeOnce()
        {
            if (_animator == null)
                _animator = GetComponent<Animator>();

            //Don't run the rest if not in play mode.
            if (!ApplicationState.IsPlaying())
                return;

            if (!_canSynchronizeAnimator)
            {
                //Debug.LogWarning("Animator is null or not enabled; unable to initialize for animator. Use SetAnimator if animator was changed or enable the animator.");
                return;
            }

            //Speed.
            _speed = _animator.speed;

            //Build layer weights.
            _layerWeights = new float[_animator.layerCount];
            for (int i = 0; i < _layerWeights.Length; i++)
                _layerWeights[i] = _animator.GetLayerWeight(i);

            _parameterDetails.Clear();
            _bools.Clear();
            _floats.Clear();
            _ints.Clear();
            //Create a parameter detail for each parameter that can be synchronized.
            foreach (AnimatorControllerParameter item in _animator.parameters)
            {
                bool process = !_animator.IsParameterControlledByCurve(item.name);
                if (process)
                {
                    //Over 250 parameters; who would do this!?
                    if (_parameterDetails.Count == 240)
                    {
                        NetworkManager.LogError($"Parameter {item.name} exceeds the allowed 240 parameter count and is being ignored.");
                        continue;
                    }

                    int typeIndex = 0;
                    //Bools.
                    if (item.type == AnimatorControllerParameterType.Bool)
                    {
                        typeIndex = _bools.Count;
                        _bools.Add(_animator.GetBool(item.nameHash));
                    }
                    //Floats.
                    else if (item.type == AnimatorControllerParameterType.Float)
                    {
                        typeIndex = _floats.Count;
                        _floats.Add(_animator.GetFloat(item.name));
                    }
                    //Ints.
                    else if (item.type == AnimatorControllerParameterType.Int)
                    {
                        typeIndex = _ints.Count;
                        _ints.Add(_animator.GetInteger(item.nameHash));
                    }
                    //Triggers.
                    else if (item.type == AnimatorControllerParameterType.Trigger)
                    {
                        /* Triggers aren't persistent so they don't use stored values
                         * but I do need to make a parameter detail to track the hash. */
                        typeIndex = -1;
                    }

                    _parameterDetails.Add(new(item, (byte)typeIndex));
                }
            }
        }

        /// <summary>
        /// Sets which animator to use. You must call this with the appropriate animator on all clients and server. This change is not automatically synchronized.
        /// </summary>
        /// <param name = "animator"></param>
        public void SetAnimator(Animator animator)
        {
            //No update required.
            if (animator == _lastAnimator)
                return;

            _animator = animator;
            InitializeOnce();
            _lastAnimator = animator;
        }

        /// <summary>
        /// Sets which controller to use. You must call this with the appropriate controller on all clients and server. This change is not automatically synchronized.
        /// </summary>
        /// <param name = "controller"></param>
        public void SetController(RuntimeAnimatorController controller)
        {
            //No update required.
            if (controller == _lastController)
                return;

            _animator.runtimeAnimatorController = controller;
            InitializeOnce();
            _lastController = controller;
        }

        /// <summary>
        /// Checks to send animator data from server to clients.
        /// </summary>
        private void CheckSendToServer()
        {
            using (_pm_CheckSendToServer.Auto())
            {
                //Cannot send to server if is server or not client.
                if (IsServerStarted || !IsClientInitialized)
                    return;
                //Cannot send to server if not client authoritative or don't have authority.
                if (!ClientAuthoritative || !IsOwner)
                    return;

                /* If there are updated parameters to send.
                 * Don't really need to worry about mtu here
                 * because there's no way the sent bytes are
                 * ever going to come close to the mtu
                 * when sending a single update. */
                if (AnimatorUpdated(out ArraySegment<byte> updatedBytes, _forceAllOnTimed))
                    ServerAnimatorUpdated(updatedBytes);

                _forceAllOnTimed = false;
            }
        }

        /// <summary>
        /// Checks to send animator data from server to clients.
        /// </summary>
        private void CheckSendToClients()
        {
            using (_pm_CheckSendToClients.Auto())
            {
                //Cannot send to clients if not server initialized.
                if (!IsServerInitialized)
                    return;

                bool sendFromServer;
                //If client authoritative.
                if (ClientAuthoritative)
                {
                    //If has no owner then use latest values on server.
                    if (!Owner.IsValid)
                    {
                        sendFromServer = true;
                    }
                    //If has a owner.
                    else
                    {
                        //If is owner then send latest values on server.
                        if (IsOwner)
                        {
                            sendFromServer = true;
                        }
                        //Not owner.
                        else
                        {
                            //Haven't received any data from clients, cannot send yet.
                            if (_clientAuthoritativeUpdates.BufferCount == 0)
                            {
                                return;
                            }
                            //Data was received from client; check eligibility to send it.
                            else
                            {
                                /* If forceAll is true then the latest values on
                                 * server must be used, rather than what was received
                                 * from client. This can occur if the client is possibly
                                 * trying to use an attack or if the client is
                                 * excessively sending updates. To prevent relaying that
                                 * same data to others the server will send it's current
                                 * animator settings in this scenario. */
                                if (_clientAuthoritativeUpdates.ForceAll)
                                {
                                    sendFromServer = true;
                                    _clientAuthoritativeUpdates.Reset();
                                }
                                else
                                {
                                    sendFromServer = false;
                                }
                            }
                        }
                    }
                }
                //Not client authoritative, always send from server.
                else
                {
                    sendFromServer = true;
                }

                /* If client authoritative then use what was received from clients
                 * if data exist. */
                if (!sendFromServer)
                {
                    byte[] buffer = null;
                    int bufferLength = 0;
                    for (int i = 0; i < _clientAuthoritativeUpdates.BufferCount; i++)
                    {
                        _clientAuthoritativeUpdates.GetBuffer(i, ref buffer, ref bufferLength);

                        //If null was returned then something went wrong.
                        if (buffer == null || bufferLength == 0)
                            continue;

                        SendSegment(new(buffer, 0, bufferLength));
                    }
                    //Reset client auth buffer.
                    _clientAuthoritativeUpdates.Reset();
                }
                //Sending from server, send what's changed.
                else
                {
                    if (AnimatorUpdated(out ArraySegment<byte> updatedBytes, _forceAllOnTimed))
                        SendSegment(updatedBytes);

                    _forceAllOnTimed = false;
                }

                //Sends segment to clients
                void SendSegment(ArraySegment<byte> data)
                {
                    foreach (NetworkConnection nc in Observers)
                    {
                        //If to not send to owner.
                        if (!_sendToOwner && nc == Owner)
                            continue;
                        TargetAnimatorUpdated(nc, data);
                    }
                }
            }
        }

        /// <summary>
        /// Smooths floats on clients.
        /// </summary>
        private void SmoothFloats()
        {
            using (_pm_SmoothFloats.Auto())
            {
                //Don't need to smooth on authoritative client.
                if (!_canSmoothFloats)
                    return;
                //Nothing to smooth.
                if (_smoothedFloats.Count == 0)
                    return;

                float deltaTime = Time.deltaTime;

                List<int> finishedEntries = new();

                /* Cycle through each target float and move towards it.
                 * Once at a target float mark it to be removed from floatTargets. */
                foreach (KeyValuePair<int, SmoothedFloat> item in _smoothedFloats)
                {
                    float current = _animator.GetFloat(item.Key);
                    float next = Mathf.MoveTowards(current, item.Value.Target, item.Value.Rate * deltaTime);
                    _animator.SetFloat(item.Key, next);

                    if (next == item.Value.Target)
                        finishedEntries.Add(item.Key);
                }

                //Remove finished entries from dictionary.
                for (int i = 0; i < finishedEntries.Count; i++)
                    _smoothedFloats.Remove(finishedEntries[i]);
            }
        }

        /// <summary>
        /// Returns if animator is updated and bytes of updated values.
        /// </summary>
        /// <returns></returns>
        private bool AnimatorUpdated(out ArraySegment<byte> updatedBytes, bool forceAll = false)
        {
            using (_pm_AnimatorUpdated.Auto())
            {
                updatedBytes = default;
                //Something isn't setup right.
                if (_layerWeights == null)
                    return false;
                //Reset the writer.
                _writer.Clear();

                /* Every time a parameter is updated a byte is added
                 * for it's index, this is why requiredBytes increases
                 * by 1 when a value updates. ChangedParameter contains
                 * the index updated and the new value. The requiresBytes
                 * is increased also by however many bytes are required
                 * for the type which has changed. Some types use special parameter
                 * detail indexes, such as layer weights; these can be found under const. */
                for (byte parameterIndex = 0; parameterIndex < _parameterDetails.Count; parameterIndex++)
                {
                    ParameterDetail pd = _parameterDetails[parameterIndex];
                    /* Bool. */
                    if (pd.ControllerParameter.type == AnimatorControllerParameterType.Bool)
                    {
                        bool next = _animator.GetBool(pd.Hash);
                        //If changed.
                        if (forceAll || _bools[pd.TypeIndex] != next)
                        {
                            _writer.WriteUInt8Unpacked(parameterIndex);
                            _writer.WriteBoolean(next);
                            _bools[pd.TypeIndex] = next;
                        }
                    }
                    /* Float. */
                    else if (pd.ControllerParameter.type == AnimatorControllerParameterType.Float)
                    {
                        float next = _animator.GetFloat(pd.Hash);
                        //If changed.
                        if (forceAll || _floats[pd.TypeIndex] != next)
                        {
                            _writer.WriteUInt8Unpacked(parameterIndex);
                            _writer.WriteSingle(next);
                            _floats[pd.TypeIndex] = next;
                        }
                    }
                    /* Int. */
                    else if (pd.ControllerParameter.type == AnimatorControllerParameterType.Int)
                    {
                        int next = _animator.GetInteger(pd.Hash);
                        //If changed.
                        if (forceAll || _ints[pd.TypeIndex] != next)
                        {
                            _writer.WriteUInt8Unpacked(parameterIndex);
                            _writer.WriteInt32(next);
                            _ints[pd.TypeIndex] = next;
                        }
                    }
                }

                /* Don't need to force trigger sends since
                 * they're one-shots. */
                for (int i = 0; i < _triggerUpdates.Count; i++)
                {
                    _writer.WriteUInt8Unpacked(_triggerUpdates[i].ParameterIndex);
                    _writer.WriteBoolean(_triggerUpdates[i].Setting);
                }
                _triggerUpdates.Clear();

                /* States. */
                if (forceAll)
                {
                    //Add all layers to layer states.
                    for (int i = 0; i < _animator.layerCount; i++)
                        _unsynchronizedLayerStates[i] = new(Time.frameCount);
                }

                /* Only iterate if the collection has values. This is to avoid some
                 * unnecessary caching when collection is empty. */
                if (_unsynchronizedLayerStates.Count > 0)
                {
                    int frameCount = Time.frameCount;
                    List<int> sentLayers = CollectionCaches<int>.RetrieveList();
                    //Go through each layer which needs to be synchronized.
                    foreach (KeyValuePair<int, StateChange> item in _unsynchronizedLayerStates)
                    {
                        /* If a frame has not passed since the state was created
                         * then do not send it until next tick. State changes take 1 frame
                         * to be processed by Unity, this check ensures that. */
                        if (frameCount == item.Value.FrameCount)
                            continue;

                        //Add to layers being sent. This is so they can be removed from the collection later.
                        sentLayers.Add(item.Key);
                        int layerIndex = item.Key;
                        StateChange sc = item.Value;
                        //If a regular state change.
                        if (!sc.IsCrossfade)
                        {
                            if (ReturnCurrentLayerState(out int stateHash, out float normalizedTime, layerIndex))
                            {
                                _writer.WriteUInt8Unpacked(STATE);
                                _writer.WriteUInt8Unpacked((byte)layerIndex);
                                //Current hash will always be too large to compress.
                                _writer.WriteInt32Unpacked(stateHash);
                                _writer.WriteSingle(normalizedTime);
                            }
                        }
                        //When it's a crossfade then send crossfade data.
                        else
                        {
                            _writer.WriteUInt8Unpacked(CROSSFADE);
                            _writer.WriteUInt8Unpacked((byte)layerIndex);
                            //Current hash will always be too large to compress.
                            _writer.WriteInt32(sc.Hash);
                            _writer.WriteBoolean(sc.FixedTime);
                            //Times usually can be compressed.
                            _writer.WriteSingle(sc.DurationTime);
                            _writer.WriteSingle(sc.OffsetTime);
                            _writer.WriteSingle(sc.NormalizedTransitionTime);
                        }
                    }

                    if (sentLayers.Count > 0)
                    {
                        for (int i = 0; i < sentLayers.Count; i++)
                            _unsynchronizedLayerStates.Remove(sentLayers[i]);
                        //Store cache.
                        CollectionCaches<int>.Store(sentLayers);
                    }
                }

                /* Layer weights. */
                for (int layerIndex = 0; layerIndex < _layerWeights.Length; layerIndex++)
                {
                    float next = _animator.GetLayerWeight(layerIndex);
                    if (forceAll || _layerWeights[layerIndex] != next)
                    {
                        _writer.WriteUInt8Unpacked(LAYER_WEIGHT);
                        _writer.WriteUInt8Unpacked((byte)layerIndex);
                        _writer.WriteSingle(next);
                        _layerWeights[layerIndex] = next;
                    }
                }

                /* Speed is similar to layer weights but we don't need the index,
                 * only the indicator and value. */
                float speedNext = _animator.speed;
                if (forceAll || _speed != speedNext)
                {
                    _writer.WriteUInt8Unpacked(SPEED);
                    _writer.WriteSingle(speedNext);
                    _speed = speedNext;
                }

                //Nothing to update.
                if (_writer.Position == 0)
                    return false;

                updatedBytes = _writer.GetArraySegment();
                return true;
            }
        }

        /// <summary>
        /// Applies changed parameters to the animator.
        /// </summary>
        /// <param name = "changedParameters"></param>
        private void ApplyParametersUpdated(ref ArraySegment<byte> updatedParameters)
        {
            using (_pm_ApplyParametersUpdated.Auto())
            {
                if (!_canSynchronizeAnimator)
                    return;
                if (_layerWeights == null)
                    return;
                if (updatedParameters.Count == 0)
                    return;

                PooledReader reader = ReaderPool.Retrieve(updatedParameters, NetworkManager);

                try
                {
                    while (reader.Remaining > 0)
                    {
                        byte parameterIndex = reader.ReadUInt8Unpacked();
                        //Layer weight
                        if (parameterIndex == LAYER_WEIGHT)
                        {
                            byte layerIndex = reader.ReadUInt8Unpacked();
                            float value = reader.ReadSingle();
                            _animator.SetLayerWeight((int)layerIndex, value);
                        }
                        //Speed.
                        else if (parameterIndex == SPEED)
                        {
                            float value = reader.ReadSingle();
                            _animator.speed = value;
                        }
                        //State.
                        else if (parameterIndex == STATE)
                        {
                            byte layerIndex = reader.ReadUInt8Unpacked();
                            //Hashes will always be too large to compress.
                            int hash = reader.ReadInt32Unpacked();
                            float normalizedTime = reader.ReadSingle();
                            //Play results.
                            _animator.Play(hash, layerIndex, normalizedTime);
                        }
                        //Crossfade.
                        else if (parameterIndex == CROSSFADE)
                        {
                            byte layerIndex = reader.ReadUInt8Unpacked();
                            //Hashes will always be too large to compress.
                            int hash = reader.ReadInt32();
                            bool useFixedTime = reader.ReadBoolean();
                            //Get time values.
                            float durationTime = reader.ReadSingle();
                            float offsetTime = reader.ReadSingle();
                            float normalizedTransitionTime = reader.ReadSingle();
                            //If using fixed.
                            if (useFixedTime)
                                _animator.CrossFadeInFixedTime(hash, durationTime, layerIndex, offsetTime, normalizedTransitionTime);
                            else
                                _animator.CrossFade(hash, durationTime, layerIndex, offsetTime, normalizedTransitionTime);
                        }
                        //Not a predetermined index, is an actual parameter.
                        else
                        {
                            AnimatorControllerParameterType acpt = _parameterDetails[parameterIndex].ControllerParameter.type;
                            if (acpt == AnimatorControllerParameterType.Bool)
                            {
                                bool value = reader.ReadBoolean();
                                _animator.SetBool(_parameterDetails[parameterIndex].Hash, value);
                            }
                            //Float.
                            else if (acpt == AnimatorControllerParameterType.Float)
                            {
                                float value = reader.ReadSingle();
                                //If able to smooth floats.
                                if (_canSmoothFloats)
                                {
                                    float currentValue = _animator.GetFloat(_parameterDetails[parameterIndex].Hash);
                                    float past = (float)TimeManager.TickDelta;
                                    //float past = _synchronizeInterval + INTERPOLATION;
                                    float rate = Mathf.Abs(currentValue - value) / past;
                                    _smoothedFloats[_parameterDetails[parameterIndex].Hash] = new(rate, value);
                                }
                                else
                                {
                                    _animator.SetFloat(_parameterDetails[parameterIndex].Hash, value);
                                }
                            }
                            //Integer.
                            else if (acpt == AnimatorControllerParameterType.Int)
                            {
                                int value = reader.ReadInt32();
                                _animator.SetInteger(_parameterDetails[parameterIndex].Hash, value);
                            }
                            //Trigger.
                            else if (acpt == AnimatorControllerParameterType.Trigger)
                            {
                                bool value = reader.ReadBoolean();
                                if (value)
                                    _animator.SetTrigger(_parameterDetails[parameterIndex].Hash);
                                else
                                    _animator.ResetTrigger(_parameterDetails[parameterIndex].Hash);
                            }
                            //Unhandled.
                            else
                            {
                                NetworkManager.LogWarning($"Unhandled parameter type of {acpt}.");
                            }
                        }
                    }
                }
                catch
                {
                    NetworkManager.LogWarning("An error occurred while applying updates. This may occur when malformed data is sent or when you change the animator or controller but not on all connections.");
                }
                finally
                {
                    reader?.Store();
                }
            }
        }

        /// <summary>
        /// Outputs the current state and time for a layer. Returns true if stateHash is not 0.
        /// </summary>
        /// <param name = "stateHash"></param>
        /// <param name = "normalizedTime"></param>
        /// <param name = "results"></param>
        /// <param name = "layerIndex"></param>
        /// <returns></returns>
        private bool ReturnCurrentLayerState(out int stateHash, out float normalizedTime, int layerIndex)
        {
            stateHash = 0;
            normalizedTime = 0f;

            if (!_canSynchronizeAnimator)
                return false;

            AnimatorStateInfo st = _animator.GetCurrentAnimatorStateInfo(layerIndex);
            stateHash = st.fullPathHash;
            normalizedTime = st.normalizedTime;

            return stateHash != 0;
        }

        /// <summary>
        /// Immediately sends all variables and states of layers.
        /// This is a very bandwidth intensive operation.
        /// </summary>
        public void SendAll()
        {
            _forceAllOnTimed = true;
        }

        #region Play.
        /// <summary>
        /// Plays a state.
        /// </summary>
        public void Play(string name)
        {
            Play(Animator.StringToHash(name));
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void Play(int hash)
        {
            for (int i = 0; i < _animator.layerCount; i++)
                Play(hash, i, 0f);
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void Play(string name, int layer)
        {
            Play(Animator.StringToHash(name), layer);
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void Play(int hash, int layer)
        {
            Play(hash, layer, 0f);
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void Play(string name, int layer, float normalizedTime)
        {
            Play(Animator.StringToHash(name), layer, normalizedTime);
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void Play(int hash, int layer, float normalizedTime)
        {
            if (!_canSynchronizeAnimator)
                return;
            if (_animator.HasState(layer, hash) || hash == 0)
            {
                _animator.Play(hash, layer, normalizedTime);
                _unsynchronizedLayerStates[layer] = new(Time.frameCount);
            }
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void PlayInFixedTime(string name, float fixedTime)
        {
            PlayInFixedTime(Animator.StringToHash(name), fixedTime);
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void PlayInFixedTime(int hash, float fixedTime)
        {
            for (int i = 0; i < _animator.layerCount; i++)
                PlayInFixedTime(hash, i, fixedTime);
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void PlayInFixedTime(string name, int layer, float fixedTime)
        {
            PlayInFixedTime(Animator.StringToHash(name), layer, fixedTime);
        }

        /// <summary>
        /// Plays a state.
        /// </summary>
        public void PlayInFixedTime(int hash, int layer, float fixedTime)
        {
            if (!_canSynchronizeAnimator)
                return;
            if (_animator.HasState(layer, hash) || hash == 0)
            {
                _animator.PlayInFixedTime(hash, layer, fixedTime);
                _unsynchronizedLayerStates[layer] = new(Time.frameCount);
            }
        }
        #endregion

        #region Crossfade.
        /// <summary>
        /// Creates a crossfade from the current state to any other state using normalized times.
        /// </summary>
        /// <param name = "stateName"></param>
        /// <param name = "normalizedTransitionDuration"></param>
        /// <param name = "layer"></param>
        /// <param name = "normalizedTimeOffset"></param>
        /// <param name = "normalizedTransitionTime"></param>
        public void CrossFade(string stateName, float normalizedTransitionDuration, int layer, float normalizedTimeOffset = float.NegativeInfinity, float normalizedTransitionTime = 0.0f)
        {
            CrossFade(Animator.StringToHash(stateName), normalizedTransitionDuration, layer, normalizedTimeOffset, normalizedTransitionTime);
        }

        /// <summary>
        /// Creates a crossfade from the current state to any other state using normalized times.
        /// </summary>
        /// <param name = "hash"></param>
        /// <param name = "normalizedTransitionDuration"></param>
        /// <param name = "layer"></param>
        /// <param name = "normalizedTimeOffset"></param>
        /// <param name = "normalizedTransitionTime"></param>
        public void CrossFade(int hash, float normalizedTransitionDuration, int layer, float normalizedTimeOffset = 0.0f, float normalizedTransitionTime = 0.0f)
        {
            if (!_canSynchronizeAnimator)
                return;
            if (_animator.HasState(layer, hash) || hash == 0)
            {
                _animator.CrossFade(hash, normalizedTransitionDuration, layer, normalizedTimeOffset, normalizedTransitionTime);
                _unsynchronizedLayerStates[layer] = new(Time.frameCount, hash, false, normalizedTransitionDuration, normalizedTimeOffset, normalizedTransitionTime);
            }
        }

        /// <summary>
        /// Creates a crossfade from the current state to any other state using times in seconds.
        /// </summary>
        /// <param name = "stateName"></param>
        /// <param name = "fixedTransitionDuration"></param>
        /// <param name = "layer"></param>
        /// <param name = "fixedTimeOffset"></param>
        /// <param name = "normalizedTransitionTime"></param>
        public void CrossFadeInFixedTime(string stateName, float fixedTransitionDuration, int layer, float fixedTimeOffset = 0.0f, float normalizedTransitionTime = 0.0f)
        {
            CrossFadeInFixedTime(Animator.StringToHash(stateName), fixedTransitionDuration, layer, fixedTimeOffset, normalizedTransitionTime);
        }

        /// <summary>
        /// Creates a crossfade from the current state to any other state using times in seconds.
        /// </summary>
        /// <param name = "hash"></param>
        /// <param name = "fixedTransitionDuration"></param>
        /// <param name = "layer"></param>
        /// <param name = "fixedTimeOffset"></param>
        /// <param name = "normalizedTransitionTime"></param>
        public void CrossFadeInFixedTime(int hash, float fixedTransitionDuration, int layer, float fixedTimeOffset = 0.0f, float normalizedTransitionTime = 0.0f)
        {
            if (!_canSynchronizeAnimator)
                return;
            if (_animator.HasState(layer, hash) || hash == 0)
            {
                _animator.CrossFadeInFixedTime(hash, fixedTransitionDuration, layer, fixedTimeOffset, normalizedTransitionTime);
                _unsynchronizedLayerStates[layer] = new(Time.frameCount, hash, true, fixedTransitionDuration, fixedTimeOffset, normalizedTransitionTime);
            }
        }
        #endregion

        #region Triggers.
        /// <summary>
        /// Sets a trigger on the animator and sends it over the network.
        /// </summary>
        /// <param name = "hash"></param>
        public void SetTrigger(int hash)
        {
            if (!_canSynchronizeAnimator)
                return;
            UpdateTrigger(hash, true);
        }

        /// <summary>
        /// Sets a trigger on the animator and sends it over the network.
        /// </summary>
        /// <param name = "hash"></param>
        public void SetTrigger(string name)
        {
            SetTrigger(Animator.StringToHash(name));
        }

        /// <summary>
        /// Resets a trigger on the animator and sends it over the network.
        /// </summary>
        /// <param name = "hash"></param>
        public void ResetTrigger(int hash)
        {
            UpdateTrigger(hash, false);
        }

        /// <summary>
        /// Resets a trigger on the animator and sends it over the network.
        /// </summary>
        /// <param name = "hash"></param>
        public void ResetTrigger(string name)
        {
            ResetTrigger(Animator.StringToHash(name));
        }

        /// <summary>
        /// Updates a trigger, sets or resets.
        /// </summary>
        /// <param name = "set"></param>
        private void UpdateTrigger(int hash, bool set)
        {
            if (!_canSynchronizeAnimator)
                return;

            bool clientAuth = ClientAuthoritative;
            //If there is an owner perform checks.
            if (Owner.IsValid)
            {
                //If client auth and not owner.
                if (clientAuth && !IsOwner)
                    return;
            }
            //There is no owner.
            else
            {
                if (!IsServerStarted)
                    return;
            }

            //Update locally.
            if (set)
                _animator.SetTrigger(hash);
            else
                _animator.ResetTrigger(hash);

            /* Can send if any of the following are true:
             * ClientAuth + Owner.
             * ClientAuth + No Owner + IsServer
             * !ClientAuth + IsServer. */
            bool canSend = (clientAuth && IsOwner) || (clientAuth && !Owner.IsValid) || (!clientAuth && IsServerStarted);

            //Only queue a send if proper side.
            if (canSend)
            {
                for (byte i = 0; i < _parameterDetails.Count; i++)
                {
                    if (_parameterDetails[i].Hash == hash)
                    {
                        _triggerUpdates.Add(new(i, set));
                        return;
                    }
                }
                //Fall through, hash not found.
                NetworkManager.LogWarning($"Hash {hash} not found while trying to update a trigger.");
            }
        }
        #endregion

        #region Remote actions.
        /// <summary>
        /// Called on clients to receive an animator update.
        /// </summary>
        /// <param name = "data"></param>
        [TargetRpc(ValidateTarget = false)]
        private void TargetAnimatorUpdated(NetworkConnection connection, ArraySegment<byte> data)
        {
            if (!_canSynchronizeAnimator)
                return;

            //If receiver is client host then do nothing, clientHost need not process.
            if (IsServerInitialized && connection.IsLocalClient)
                return;

            bool clientAuth = ClientAuthoritative;
            bool isOwner = IsOwner;
            /* If set for client auth and owner then do not process.
             * This could be the case if an update was meant to come before
             * ownership gain but came out of late due to out of order when using unreliable.
             * Cannot check sendToOwner given clients may not
             * always be aware of owner depending on ShareIds setting. */
            if (clientAuth && isOwner)
                return;
            /* If not client auth and not to send to owner, and is owner
             * then also return. */
            if (!clientAuth && !_sendToOwner && isOwner)
                return;

            ReceivedServerData rd = new(data);
            _fromServerBuffer.Enqueue(rd);

            if (_startTick == 0)
                _startTick = TimeManager.LocalTick + _interpolation;
        }

        /// <summary>
        /// Called on server to receive an animator update.
        /// </summary>
        /// <param name = "data"></param>
        [ServerRpc]
        private void ServerAnimatorUpdated(ArraySegment<byte> data)
        {
            if (!_canSynchronizeAnimator)
                return;
            if (!ClientAuthoritative)
            {
                Owner.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection Id {Owner.ClientId} has been kicked for trying to update this object without client authority.");
                return;
            }

            /* Server does not need to apply interpolation.
             * Even as clientHost when CSP is being used the
             * clientHost will always be on the latest tick.
             * Spectators on the other hand will remain behind
             * a little depending on their components interpolation. */
            ApplyParametersUpdated(ref data);
            _clientAuthoritativeUpdates.AddToBuffer(ref data);
        }
        #endregion

        #region Editor.
        #if UNITY_EDITOR
        protected override void Reset()
        {
            base.Reset();
            if (_animator == null)
                SetAnimator(GetComponent<Animator>());
        }
        #endif
        #endregion
    }
}