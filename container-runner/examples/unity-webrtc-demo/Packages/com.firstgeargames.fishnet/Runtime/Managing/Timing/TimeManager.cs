#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.Connection;
using FishNet.Serializing;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using FishNet.Managing.Observing;
using FishNet.Managing.Predicting;
using FishNet.Managing.Statistic;
using FishNet.Object;
using Unity.Profiling;
using UnityEngine;
using SystemStopwatch = System.Diagnostics.Stopwatch;

namespace FishNet.Managing.Timing
{
    /// <summary>
    /// Provides data and actions for network time and tick based systems.
    /// </summary>
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Manager/TimeManager")]
    public sealed partial class TimeManager : MonoBehaviour
    {
        #region Types.
        /// <summary>
        /// How networking timing is performed.
        /// </summary>
        private enum TimingType
        {
            /// <summary>
            /// Send and read data on tick.
            /// </summary>
            Tick = 0,
            /// <summary>
            /// Send and read data as soon as possible. This does not include built-in components, which will still run on tick.
            /// </summary>
            Variable = 1
        }

        /// <summary>
        /// When OnUpdate is performed.
        /// </summary>
        private enum UpdateOrder : byte
        {
            BeforeTick = 0,
            AfterTick = 1
        }
        #endregion

        #region Public.
        /// <summary>
        /// NetworkManager used with this.
        /// </summary>
        public NetworkManager NetworkManager { get; private set; }
        /// <summary>
        /// Called when the local clients ping is updated.
        /// </summary>
        public event Action<long> OnRoundTripTimeUpdated;
        /// <summary>
        /// Called right before a tick occurs, as well before data is read.
        /// </summary>
        public event Action OnPreTick;
        /// <summary>
        /// Called after PreTick and before OnPostTick, most similar to FixedUpdate. This is commonly where you run replicate or other network.
        /// </summary>
        public event Action OnTick;
        /// <summary>
        /// When using TimeManager for physics timing, this is called immediately before physics simulation will occur for the tick.
        /// While using Unity for physics timing, this is called during FixedUpdate.
        /// This may be useful if you wish to run physics differently for stacked scenes.
        /// </summary>
        public event Action<float> OnPrePhysicsSimulation;
        /// <summary>
        /// When using TimeManager for physics timing, this is called immediately after the physics simulation has occured for the tick.
        /// While using Unity for physics timing, this is called during Update, only if a physics frame.
        /// This may be useful if you wish to run physics differently for stacked scenes.
        /// </summary>
        public event Action<float> OnPostPhysicsSimulation;
        /// <summary>
        /// Called after a tick occurs; physics would have simulated if using PhysicsMode.TimeManager.
        /// </summary>
        public event Action OnPostTick;
        /// <summary>
        /// Called when MonoBehaviours call Update.
        /// </summary>
        public event Action OnUpdate;
        /// <summary>
        /// Called when MonoBehaviours call LateUpdate.
        /// </summary>
        public event Action OnLateUpdate;
        /// <summary>
        /// Called when MonoBehaviours call FixedUpdate.
        /// </summary>
        public event Action OnFixedUpdate;
        /// <summary>
        /// How many ticks must pass to update timing.
        /// </summary>
        internal uint TimingTickInterval => _tickRate;
        /// <summary>
        /// RoundTripTime in milliseconds. This value includes latency from the tick rate.
        /// </summary>
        public long RoundTripTime { get; private set; }
        /// <summary>
        /// Returns half value of RoundTripTime rounded to nearest whole.
        /// </summary>
        public long HalfRoundTripTime => (long)Math.Round((double)RoundTripTime / 2d);

        /// <summary>
        /// True if multiple ticks had occured within the past specified time.
        /// </summary>
        internal bool HasMultipleTicksOccurred(float timeSinceMultipleTicks)
        {
            return Time.unscaledTime - _lastMultipleTicksTime < timeSinceMultipleTicks;
        }

        /// <summary>
        /// Tick on the last received packet, be it from server or client.
        /// </summary>
        public EstimatedTick LastPacketTick { get; internal set; } = new();
        /// <summary>
        /// Current approximate network tick as it is on server.
        /// When running as client only this is an approximation to what the server tick is.
        /// The value of this field may increase and decrease as timing adjusts.
        /// This value is reset upon disconnecting.
        /// Tick can be used to get the server time by using TicksToTime().
        /// Use LocalTick for values that only increase.
        /// </summary>
        public uint Tick { get; internal set; }
        /// <summary>
        /// A fixed deltaTime for TickRate.
        /// </summary>
        [HideInInspector]
        public double TickDelta { get; private set; }
        /// <summary>
        /// True if the TimeManager will or has ticked this frame.
        /// </summary>
        public bool FrameTicked { get; private set; }
        /// <summary>
        /// How long the local server has been connected.
        /// </summary>
        public float ServerUptime { get; private set; }
        /// <summary>
        /// How long the local client has been connected.
        /// </summary>
        public float ClientUptime { get; private set; }
        #endregion

        #region Serialized.
        /// <summary>
        /// When to invoke OnUpdate and other Unity callbacks relayed by the TimeManager.
        /// </summary>
        [Tooltip("When to invoke OnUpdate and other Unity callbacks relayed by the TimeManager.")]
        [SerializeField]
        private UpdateOrder _updateOrder = UpdateOrder.BeforeTick;
        /// <summary>
        /// Timing for sending and receiving data.
        /// </summary>
        [Tooltip("Timing for sending and receiving data.")]
        [SerializeField]
        private TimingType _timingType = TimingType.Tick;
        /// <summary>
        /// While true clients may drop local ticks if their devices are unable to maintain the tick rate.
        /// This could result in a temporary desynchronization but will prevent the client falling further behind on ticks by repeatedly running the logic cycle multiple times per frame.
        /// </summary>
        [Tooltip("While true clients may drop local ticks if their devices are unable to maintain the tick rate. This could result in a temporary desynchronization but will prevent the client falling further behind on ticks by repeatedly running the logic cycle multiple times per frame.")]
        [SerializeField]
        private bool _allowTickDropping = true;
        /// <summary>
        /// Maximum number of ticks which may occur in a single frame before remainder are dropped for the frame.
        /// </summary>
        [Tooltip("Maximum number of ticks which may occur in a single frame before remainder are dropped for the frame.")]
        [Range(1, 25)]
        [SerializeField]
        private byte _maximumFrameTicks = 3;
        /// <summary>
        /// </summary>
        [Tooltip("How many times per second the server will simulate. This does not limit server frame rate.")]
        [Range(1, 240)]
        [SerializeField]
        private ushort _tickRate = 30;
        /// <summary>
        /// How many times per second the server will simulate. This does not limit server frame rate.
        /// </summary>
        public ushort TickRate
        {
            get => _tickRate;
            private set => _tickRate = value;
        }
        /// <summary>
        /// </summary>
        [Tooltip("How often in seconds to a connections ping. This is also responsible for approximating server tick. This value does not affect prediction.")]
        [Range(1, 15)]
        [SerializeField]
        private byte _pingInterval = 1;
        /// <summary>
        /// How often in seconds to a connections ping. This is also responsible for approximating server tick. This value does not affect prediction.
        /// </summary>
        public byte PingInterval => _pingInterval;
        /// <summary>
        /// </summary>
        [Tooltip("How to perform physics.")]
        [SerializeField]
        private PhysicsMode _physicsMode = PhysicsMode.Unity;
        /// <summary>
        /// How to perform physics.
        /// </summary>
        public PhysicsMode PhysicsMode => _physicsMode;
        #endregion

        #region Private.
        /// <summary>
        /// </summary>
        private uint _localTick;
        /// <summary>
        /// A tick that is not synchronized. This value will only increment. May be used for indexing or Ids with custom logic.
        /// When called on the server Tick is returned, otherwise LocalTick is returned.
        /// This value resets upon disconnecting.
        /// </summary>
        public uint LocalTick
        {
            get => NetworkManager.IsServerStarted ? Tick : _localTick;
            private set => _localTick = value;
        }
        /// <summary>
        /// Stopwatch used for pings.
        /// </summary>
        private SystemStopwatch _pingStopwatch = new();
        /// <summary>
        /// Ticks passed since last ping.
        /// </summary>
        private uint _pingTicks;
        /// <summary>
        /// MovingAverage instance used to calculate mean ping.
        /// </summary>
        private MovingAverage _pingAverage = new(5);
        /// <summary>
        /// Accumulating frame time to determine when to increase tick.
        /// </summary>
        private double _elapsedTickTime;
        /// <summary>
        /// Internal deltaTime for clients. Controlled by the server.
        /// </summary>
        private double _adjustedTickDelta;
        /// <summary>
        /// Last frame an iteration occurred for incoming.
        /// </summary>
        private int _lastIncomingIterationFrame = -1;
        /// <summary>
        /// True if client received Pong since last ping.
        /// </summary>
        private bool _receivedPong = true;
        /// <summary>
        /// Last unscaledTime multiple ticks occurred in a single frame.
        /// </summary>
        private float _lastMultipleTicksTime;
        /// <summary>
        /// Number of TimeManagers open which are using manual physics.
        /// </summary>
        private static uint _manualPhysics;
        /// <summary>
        /// True if FixedUpdate called this frame and using Unity physics mode.
        /// </summary>
        private bool _fixedUpdateTimeStep;
        /// <summary>
        /// </summary>
        private float _physicsTimeScale = 1f;

        /// <summary>
        /// Gets the current physics time scale.
        /// </summary>
        /// <returns></returns>
        public float GetPhysicsTimeScale() => _physicsTimeScale;

        /// <summary>
        /// Sets the physics time scale.
        /// This is not automatically synchronized.
        /// </summary>
        /// <param name = "value">New value.</param>
        public void SetPhysicsTimeScale(float value)
        {
            value = Mathf.Clamp(value, 0f, float.PositiveInfinity);
            _physicsTimeScale = value;
        }

        /// <summary>
        /// </summary>
        private NetworkTrafficStatistics _networkTrafficStatistics;
        #endregion

        #region Private Profiler Markers
        private static readonly ProfilerMarker _pm_OnFixedUpdate = new("TimeManager.OnFixedUpdate()");
        private static readonly ProfilerMarker _pm_OnPostPhysicsSimulation = new("TimeManager.OnPostPhysicsSimulation(float)");
        private static readonly ProfilerMarker _pm_OnPrePhysicsSimulation = new("TimeManager.OnPrePhysicsSimulation(float)");
        private static readonly ProfilerMarker _pm_OnUpdate = new("TimeManager.OnUpdate()");
        private static readonly ProfilerMarker _pm_OnLateUpdate = new("TimeManager.OnLateUpdate(float)");
        private static readonly ProfilerMarker _pm_OnRoundTripTimeUpdated = new("TimeManager.OnRoundTripTimeUpdated(float)");
        private static readonly ProfilerMarker _pm_OnPreTick = new("TimeManager.OnPreTick()");
        private static readonly ProfilerMarker _pm_OnTick = new("TimeManager.OnTick()");
        private static readonly ProfilerMarker _pm_OnPostTick = new("TimeManager.OnPostTick()");
        private static readonly ProfilerMarker _pm_PhysicsSimulate = new("TimeManager.Physics.Simulate(float)");
        private static readonly ProfilerMarker _pm_Physics2DSimulate = new("TimeManager.Physics2D.Simulate(float)");
        #endregion

        #region Const.
        /// <summary>
        /// Value for a tick that is invalid.
        /// </summary>
        public const uint UNSET_TICK = 0;
        /// <summary>
        /// Playerprefs string to load and save user fixed time.
        /// </summary>
        private const string SAVED_FIXED_TIME_TEXT = "SavedFixedTimeFN";
        #endregion

        #if UNITY_EDITOR
        private void OnDisable()
        {
            // If closing/stopping.
            if (ApplicationState.IsQuitting())
            {
                _manualPhysics = 0;
                UnsetSimulationSettings();
            }
            else if (PhysicsMode == PhysicsMode.TimeManager)
            {
                _manualPhysics = Math.Max(0, _manualPhysics - 1);
            }
        }
        #endif

        /// <summary>
        /// Called when FixedUpdate ticks. This is called before any other script.
        /// </summary>
        internal void TickFixedUpdate()
        {
            using (_pm_OnFixedUpdate.Auto())
                OnFixedUpdate?.Invoke();

            /* Invoke onsimulation if using Unity time.
             * Otherwise let the tick cycling part invoke. */
            if (PhysicsMode == PhysicsMode.Unity)
            {
                /* If fixedUpdateTimeStep then that means
                 * FixedUpdate already called for this frame, which
                 * means a post physics should also be called.
                 * This can only happen if a FixedUpdate occurs
                 * multiple times per frame. */
                if (_fixedUpdateTimeStep)
                {
                    using (_pm_OnPostPhysicsSimulation.Auto())
                        OnPostPhysicsSimulation?.Invoke(Time.fixedDeltaTime);
                }

                _fixedUpdateTimeStep = true;

                using (_pm_OnPrePhysicsSimulation.Auto())
                    OnPrePhysicsSimulation?.Invoke(Time.fixedDeltaTime);
            }
        }

        /// <summary>
        /// Called when Update ticks. This is called before any other script.
        /// </summary>
        internal void TickUpdate()
        {
            if (NetworkManager.IsServerStarted)
                ServerUptime += Time.deltaTime;
            if (NetworkManager.IsClientStarted)
                ClientUptime += Time.deltaTime;

            bool beforeTick = _updateOrder == UpdateOrder.BeforeTick;
            if (beforeTick)
            {
                using (_pm_OnUpdate.Auto())
                    OnUpdate?.Invoke();

                MethodLogic();
            }
            else
            {
                MethodLogic();

                using (_pm_OnUpdate.Auto())
                    OnUpdate?.Invoke();
            }

            void MethodLogic()
            {
                IncreaseTick();
                /* Invoke onsimulation if using Unity time.
                 * Otherwise let the tick cycling part invoke. */
                if (PhysicsMode == PhysicsMode.Unity && _fixedUpdateTimeStep)
                {
                    _fixedUpdateTimeStep = false;

                    using (_pm_OnPostPhysicsSimulation.Auto())
                        OnPostPhysicsSimulation?.Invoke(Time.fixedDeltaTime);
                }
            }
        }

        /// <summary>
        /// Called when LateUpdate ticks. This is called after all other scripts.
        /// </summary>
        internal void TickLateUpdate()
        {
            using (_pm_OnLateUpdate.Auto())
                OnLateUpdate?.Invoke();
        }

        /// <summary>
        /// Initializes this script for use.
        /// </summary>
        internal void InitializeOnce_Internal(NetworkManager networkManager)
        {
            NetworkManager = networkManager;
            LastPacketTick.Initialize(networkManager.TimeManager);
            SetInitialValues();

            networkManager.StatisticsManager.TryGetNetworkTrafficStatistics(out _networkTrafficStatistics);
            networkManager.ServerManager.OnServerConnectionState += ServerManager_OnServerConnectionState;
            networkManager.ClientManager.OnClientConnectionState += ClientManager_OnClientConnectionState;

            AddNetworkLoops();
        }

        /// <summary>
        /// Adds network loops to gameObject.
        /// </summary>
        private void AddNetworkLoops()
        {
            // Writer.
            if (!gameObject.TryGetComponent<NetworkWriterLoop>(out _))
                gameObject.AddComponent<NetworkWriterLoop>();
            // Reader.
            if (!gameObject.TryGetComponent<NetworkReaderLoop>(out _))
                gameObject.AddComponent<NetworkReaderLoop>();
        }

        /// <summary>
        /// Called after the local client connection state changes.
        /// </summary>
        private void ClientManager_OnClientConnectionState(ClientConnectionStateArgs obj)
        {
            if (obj.ConnectionState != LocalConnectionState.Started)
            {
                if (!NetworkManager.IsServerStarted)
                    LastPacketTick.ResetTicks();
                _pingStopwatch.Stop();
                ClientUptime = 0f;

                // Only reset ticks if also not server.
                if (!NetworkManager.IsServerStarted)
                {
                    LocalTick = 0;
                    Tick = 0;
                    SetTickRate(TickRate);
                }
            }
            // Started.
            else
            {
                _pingStopwatch.Restart();
            }
        }

        /// <summary>
        /// Called after the local server connection state changes.
        /// </summary>
        private void ServerManager_OnServerConnectionState(ServerConnectionStateArgs obj)
        {
            // If no servers are running.
            if (!NetworkManager.ServerManager.IsAnyServerStarted())
            {
                LastPacketTick.ResetTicks();
                ServerUptime = 0f;
                Tick = 0;
            }
        }

        /// <summary>
        /// Sets values to use based on settings.
        /// </summary>
        private void SetInitialValues()
        {
            SetTickRate(TickRate);
            InitializePhysicsMode(PhysicsMode);
        }

        /// <summary>
        /// Sets simulation settings to Unity defaults.
        /// </summary>
        private void UnsetSimulationSettings()
        {
            SetAutomaticPhysicsSimulation(true);

            float simulationTime = PlayerPrefs.GetFloat(SAVED_FIXED_TIME_TEXT, float.MinValue);
            if (simulationTime != float.MinValue)
                Time.fixedDeltaTime = simulationTime;
        }

        /// <summary>
        /// Sets automatic physics simulation mode.
        /// </summary>
        /// <param name = "automatic"></param>
        private void SetAutomaticPhysicsSimulation(bool automatic)
        {
            #if UNITY_2022_1_OR_NEWER
            if (automatic)
            {
                Physics.simulationMode = SimulationMode.FixedUpdate;
                Physics2D.simulationMode = SimulationMode2D.FixedUpdate;
            }
            else
            {
                Physics.simulationMode = SimulationMode.Script;
                Physics2D.simulationMode = SimulationMode2D.Script;
            }
            #else
            Physics.autoSimulation = automatic;
            if (automatic)
                Physics2D.simulationMode = SimulationMode2D.FixedUpdate;
            else
                Physics2D.simulationMode = SimulationMode2D.Script;
            #endif
        }

        /// <summary>
        /// Initializes physics mode when starting.
        /// </summary>
        /// <param name = "automatic"></param>
        private void InitializePhysicsMode(PhysicsMode mode)
        {
            // Disable.
            if (mode == PhysicsMode.Disabled)
            {
                SetPhysicsMode(mode);
            }
            // Do not automatically simulate.
            else if (mode == PhysicsMode.TimeManager)
            {
                #if UNITY_EDITOR
                // Preserve user tick rate.
                PlayerPrefs.SetFloat(SAVED_FIXED_TIME_TEXT, Time.fixedDeltaTime);
                // Let the player know.
                // if (Time.fixedDeltaTime != (float)TickDelta)
                //    Debug.LogWarning("Time.fixedDeltaTime is being overriden with TimeManager.TickDelta");
                #endif
                Time.fixedDeltaTime = (float)TickDelta;
                /* Only check this if network manager
                 * is not null. It would be null via
                 * OnValidate. */
                if (NetworkManager != null)
                {
                    // If at least one time manager is already running manual physics.
                    if (_manualPhysics > 0)
                        NetworkManager.LogError($"There are multiple TimeManagers instantiated which are using manual physics. Manual physics with multiple TimeManagers is not supported.");

                    _manualPhysics++;
                }

                SetPhysicsMode(mode);
            }
            // Automatically simulate.
            else
            {
                #if UNITY_EDITOR
                float savedTime = PlayerPrefs.GetFloat(SAVED_FIXED_TIME_TEXT, float.MinValue);
                if (savedTime != float.MinValue && Time.fixedDeltaTime != savedTime)
                {
                    Debug.LogWarning("Time.fixedDeltaTime has been set back to user values.");
                    Time.fixedDeltaTime = savedTime;
                }

                PlayerPrefs.DeleteKey(SAVED_FIXED_TIME_TEXT);
                #endif
                SetPhysicsMode(mode);
            }
        }

        /// <summary>
        /// Updates physics based on which physics mode to use.
        /// </summary>
        /// <param name = "enabled"></param>
        public void SetPhysicsMode(PhysicsMode mode)
        {
            _physicsMode = mode;

            // Disable.
            if (mode == PhysicsMode.Disabled || mode == PhysicsMode.TimeManager)
                SetAutomaticPhysicsSimulation(false);
            // Automatically simulate.
            else
                SetAutomaticPhysicsSimulation(true);
        }

        #region PingPong.
        /// <summary>
        /// Modifies client ping based on LocalTick and clientTIck.
        /// </summary>
        /// <param name = "clientTick"></param>
        internal void ModifyPing(uint clientTick)
        {
            uint tickDifference = LocalTick - clientTick;
            _pingAverage.ComputeAverage(tickDifference);
            double averageInTime = _pingAverage.Average * TickDelta * 1000;
            RoundTripTime = (long)Math.Round(averageInTime);
            _receivedPong = true;

            using (_pm_OnRoundTripTimeUpdated.Auto())
                OnRoundTripTimeUpdated?.Invoke(RoundTripTime);
        }

        /// <summary>
        /// Sends a ping to the server.
        /// </summary>
        private void TrySendPing(uint? tickOverride = null)
        {
            byte pingInterval = PingInterval;

            /* How often client may send ping is based on if
             * the server responded to the last ping.
             * A response may not be received if the server
             * believes the client is pinging too fast, or if the
             * client is having difficulties reaching the server. */
            long requiredTime = pingInterval * 1000;
            float multiplier = _receivedPong ? 1f : 1.5f;

            requiredTime = (long)(requiredTime * multiplier);
            uint requiredTicks = TimeToTicks(pingInterval * multiplier);

            _pingTicks++;
            /* We cannot just consider time because ticks might run slower
             * from adjustments. We also cannot only consider ticks because
             * they might run faster from adjustments. Therefor require both
             * to have pass checks. */
            if (_pingTicks < requiredTicks || _pingStopwatch.ElapsedMilliseconds < requiredTime)
                return;

            _pingTicks = 0;
            _pingStopwatch.Restart();
            // Unset receivedPong, wait for new response.
            _receivedPong = false;

            uint tick = tickOverride == null ? LocalTick : tickOverride.Value;
            PooledWriter writer = WriterPool.Retrieve();
            writer.WritePacketIdUnpacked(PacketId.PingPong);
            writer.WriteTickUnpacked(tick);

            #if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.PingPong, string.Empty, writer.Length, gameObject: null, asServer: false);
            #endif

            NetworkManager.TransportManager.SendToServer((byte)Channel.Unreliable, writer.GetArraySegment());
            writer.Store();
        }

        /// <summary>
        /// Sends a pong to a client.
        /// </summary>
        internal void SendPong(NetworkConnection conn, uint clientTick)
        {
            if (!conn.IsActive || !conn.IsAuthenticated)
                return;

            PooledWriter writer = WriterPool.Retrieve();
            writer.WritePacketIdUnpacked(PacketId.PingPong);
            writer.WriteTickUnpacked(clientTick);

            #if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.PingPong, string.Empty, writer.Length, gameObject: null, asServer: true);
            #endif

            conn.SendToClient((byte)Channel.Unreliable, writer.GetArraySegment());
            writer.Store();
        }
        #endregion

        /// <summary>
        /// Increases the tick based on simulation rate.
        /// </summary>
        private void IncreaseTick()
        {
            bool isClient = NetworkManager.IsClientStarted;
            bool isServer = NetworkManager.IsServerStarted;

            double timePerSimulation = isServer ? TickDelta : _adjustedTickDelta;
            if (timePerSimulation == 0d)
            {
                NetworkManager.LogWarning($"Simulation delta cannot be 0. Network timing will not continue.");
                return;
            }

            double time = Time.unscaledDeltaTime;

            _elapsedTickTime += time;
            FrameTicked = _elapsedTickTime >= timePerSimulation;

            // Number of ticks to occur this frame.
            int ticksCount = Mathf.FloorToInt((float)(_elapsedTickTime / timePerSimulation));
            if (ticksCount > 1)
                _lastMultipleTicksTime = Time.unscaledTime;

            if (_allowTickDropping)
            {
                // If ticks require dropping. Set exactly to maximum ticks.
                if (ticksCount > _maximumFrameTicks)
                    _elapsedTickTime = timePerSimulation * (double)_maximumFrameTicks;
            }

            bool variableTiming = _timingType == TimingType.Variable;
            bool frameTicked = FrameTicked;
            float tickDelta = (float)TickDelta * GetPhysicsTimeScale();

            do
            {
                if (frameTicked)
                {
                    using (_pm_OnPreTick.Auto())
                        OnPreTick?.Invoke();
                }

                /* This has to be called inside the loop because
                 * OnPreTick promises data hasn't been read yet.
                 * Therefor iterate must occur after OnPreTick.
                 * Iteration will only run once per frame. */
                if (frameTicked || variableTiming)
                    TryIterateData(true);

                if (frameTicked)
                {
                    // Tell predicted objecs to reconcile before OnTick.
                    NetworkManager.PredictionManager.ReconcileToStates();

                    using (_pm_OnTick.Auto())
                        OnTick?.Invoke();

                    if (PhysicsMode == PhysicsMode.TimeManager && tickDelta > 0f)
                    {
                        InvokeOnPhysicsSimulation(preSimulation: true, tickDelta);
                        SimulatePhysics(tickDelta);
                        InvokeOnPhysicsSimulation(preSimulation: false, tickDelta);
                    }

                    using (_pm_OnPostTick.Auto())
                        OnPostTick?.Invoke();
                    // After post tick send states.
                    NetworkManager.PredictionManager.SendStateUpdate();

                    /* If isClient this is the
                     * last tick during this loop. */
                    bool lastTick = _elapsedTickTime < timePerSimulation * 2d;
                    if (isClient && lastTick)
                        TrySendPing(LocalTick + 1);
                    if (NetworkManager.IsServerStarted)
                        SendTimingAdjustment();
                }

                // Send out data.
                if (frameTicked || variableTiming)
                    TryIterateData(false);

                if (frameTicked)
                {
                    _elapsedTickTime -= timePerSimulation;
                    Tick++;
                    LocalTick++;
                    
                    //Cache localTick to ObserverManager for performance.
                    NetworkManager.ObserverManager.LocalTick = LocalTick;
                }
            } while (_elapsedTickTime >= timePerSimulation);
        }

        #region Tick conversions.
        /// <summary>
        /// Returns the percentage of how far the TimeManager is into the next tick as a double.
        /// Value will return between 0d and 1d.
        /// </summary>
        /// <returns></returns>
        public double GetTickPercentAsDouble()
        {
            if (NetworkManager == null)
                return 0d;

            double percent = _elapsedTickTime / TickDelta;
            return percent;
        }

        /// <summary>
        /// Returns the current elapsed amount for the next tick.
        /// </summary>
        /// <returns></returns>
        public double GetTickElapsedAsDouble() => _elapsedTickTime;

        /// <summary>
        /// Returns the percentage of how far the TimeManager is into the next tick.
        /// Value will return between 0 and 100.
        /// </summary>
        public byte GetTickPercentAsByte()
        {
            double result = GetTickPercentAsDouble();
            return (byte)(result * 100d);
        }

        /// <summary>
        /// Converts a 0 to 100 byte value to a 0d to 1d percent value.
        /// This does not check for excessive byte values, such as anything over 100.
        /// </summary>
        public static double GetTickPercentAsDouble(byte value)
        {
            return value / 100d;
        }

        /// <summary>
        /// Returns a PreciseTick.
        /// </summary>
        /// <param name = "tick">Tick to set within the returned PreciseTick.</param>
        /// <returns></returns>
        public PreciseTick GetPreciseTick(uint tick)
        {
            if (NetworkManager == null)
                return default;

            double delta = NetworkManager.IsServerStarted ? TickDelta : _adjustedTickDelta;
            double percent = _elapsedTickTime / delta;

            return new(tick, percent);
        }

        /// <summary>
        /// Returns a PreciseTick.
        /// </summary>
        /// <param name = "tickType">Tick to use within PreciseTick.</param>
        /// <returns></returns>
        public PreciseTick GetPreciseTick(TickType tickType)
        {
            if (NetworkManager == null)
                return default;

            if (tickType == TickType.Tick)
            {
                return GetPreciseTick(Tick);
            }
            else if (tickType == TickType.LocalTick)
            {
                return GetPreciseTick(LocalTick);
            }
            else if (tickType == TickType.LastPacketTick)
            {
                return GetPreciseTick(LastPacketTick.LastRemoteTick);
            }
            else
            {
                NetworkManager.LogError($"TickType {tickType.ToString()} is unhandled.");
                return default;
            }
        }

        /// <summary>
        /// Converts current ticks to time.
        /// </summary>
        /// <param name = "tickType">TickType to compare against.</param>
        /// <returns></returns>
        public double TicksToTime(TickType tickType = TickType.LocalTick)
        {
            if (tickType == TickType.LocalTick)
            {
                return TicksToTime(LocalTick);
            }
            else if (tickType == TickType.Tick)
            {
                return TicksToTime(Tick);
            }
            else if (tickType == TickType.LastPacketTick)
            {
                return TicksToTime(LastPacketTick.LastRemoteTick);
            }
            else
            {
                NetworkManager.LogError($"TickType {tickType} is unhandled.");
                return 0d;
            }
        }

        /// <summary>
        /// Converts a PreciseTick to time.
        /// </summary>
        /// <param name = "pt">PreciseTick to convert.</param>
        /// <returns></returns>
        public double TicksToTime(PreciseTick pt)
        {
            double tickTime = TicksToTime(pt.Tick);
            double percentTime = pt.PercentAsDouble * TickDelta;
            return tickTime + percentTime;
        }

        /// <summary>
        /// Converts a number ticks to time.
        /// </summary>
        /// <param name = "ticks">Ticks to convert.</param>
        /// <returns></returns>
        public double TicksToTime(uint ticks)
        {
            return TickDelta * (double)ticks;
        }

        /// <summary>
        /// Gets time passed from currentTick to previousTick.
        /// </summary>
        /// <param name = "currentTick">The current tick.</param>
        /// <param name = "previousTick">The previous tick.</param>
        /// <returns></returns>
        public double TimePassed(uint currentTick, uint previousTick)
        {
            double multiplier;
            double result;
            if (currentTick >= previousTick)
            {
                multiplier = 1f;
                result = TicksToTime(currentTick - previousTick);
            }
            else
            {
                multiplier = -1f;
                result = TicksToTime(previousTick - currentTick);
            }

            return result * multiplier;
        }

        /// <summary>
        /// Gets time passed from Tick to preciseTick.
        /// </summary>
        /// <param name = "preciseTick">PreciseTick value to compare against.</param>
        /// <param name = "allowNegative">True to allow negative values. When false and value would be negative 0 is returned.</param>
        /// <returns></returns>
        public double TimePassed(PreciseTick preciseTick, bool allowNegative = false)
        {
            PreciseTick currentPt = GetPreciseTick(TickType.Tick);

            long tickDifference = (long)currentPt.Tick - (long)preciseTick.Tick;
            double percentDifference = currentPt.PercentAsDouble - preciseTick.PercentAsDouble;

            /* If tickDifference is less than 0 or tickDifference and percentDifference are 0 or less
             * then the result would be negative. */
            bool negativeValue = tickDifference < 0 || (tickDifference <= 0 && percentDifference <= 0);

            if (!allowNegative && negativeValue)
                return 0d;

            double tickTime = TimePassed(preciseTick.Tick, true);
            double percentTime = percentDifference * TickDelta;

            return tickTime + percentTime;
        }

        /// <summary>
        /// Gets time passed from Tick to previousTick.
        /// </summary>
        /// <param name = "previousTick">The previous tick.</param>
        /// <param name = "allowNegative">True to allow negative values. When false and value would be negative 0 is returned.</param>
        /// <returns></returns>
        public double TimePassed(uint previousTick, bool allowNegative = false)
        {
            uint currentTick = Tick;
            // Difference will be positive.
            if (currentTick >= previousTick)
            {
                return TicksToTime(currentTick - previousTick);
            }
            // Difference would be negative.
            else
            {
                if (!allowNegative)
                {
                    return 0d;
                }
                else
                {
                    double difference = TicksToTime(previousTick - currentTick);
                    return difference * -1d;
                }
            }
        }

        /// <summary>
        /// Converts time to ticks.
        /// </summary>
        /// <param name = "time">Time to convert as decimal.</param>
        /// <returns></returns>
        public uint TimeToTicks(double time, TickRounding rounding = TickRounding.RoundNearest)
        {
            double result = time / TickDelta;

            if (rounding == TickRounding.RoundNearest)
                return (uint)Math.Round(result);
            else if (rounding == TickRounding.RoundDown)
                return (uint)Math.Floor(result);
            else
                return (uint)Math.Ceiling(result);
        }

        /// <summary>
        /// Converts time to ticks.
        /// </summary>
        /// <param name = "time">Time to convert as whole (milliseconds)</param>
        /// <returns></returns>
        public uint TimeToTicks(long time, TickRounding rounding = TickRounding.RoundNearest)
        {
            double dTime = (double)time / 1000d;
            return TimeToTicks(dTime, rounding);
        }

        /// <summary>
        /// Converts time to a PreciseTick.
        /// </summary>
        /// <param name = "time">Time to convert.</param>
        /// <returns></returns>
        public PreciseTick TimeToPreciseTick(double time) => time.AsPreciseTick(TickDelta);

        /// <summary>
        /// Estimatedly converts a synchronized tick to what it would be for the local tick.
        /// </summary>
        /// <param name = "tick">Synchronized tick to convert.</param>
        /// <returns></returns>
        public uint TickToLocalTick(uint tick)
        {
            // Server will always have local and tick aligned.
            if (NetworkManager.IsServerStarted)
                return tick;

            long difference = Tick - tick;

            long result = LocalTick - difference;
            if (result <= 0)
                result = 0;

            return (uint)result;
        }

        /// <summary>
        /// Estimatedly converts a local tick to what it would be for the synchronized tick.
        /// </summary>
        /// <param name = "localTick">Local tick to convert.</param>
        /// <returns></returns>
        public uint LocalTickToTick(uint localTick)
        {
            // Server will always have local and tick aligned.
            if (NetworkManager.IsServerStarted)
                return localTick;

            long difference = LocalTick - localTick;

            long result = Tick - difference;
            if (result <= 0)
                result = 0;

            return (uint)result;
        }
        #endregion

        /// <summary>
        /// Invokes OnPreSimulation or OnPostSimulation.
        /// </summary>
        internal void InvokeOnPhysicsSimulation(bool preSimulation, float delta)
        {
            if (preSimulation)
            {
                using (_pm_OnPrePhysicsSimulation.Auto())
                    OnPrePhysicsSimulation?.Invoke(delta);
            }
            else
            {
                using (_pm_OnPostPhysicsSimulation.Auto())
                    OnPostPhysicsSimulation?.Invoke(delta);
            }
        }

        internal void SimulatePhysics(float delta)
        {
            using (_pm_PhysicsSimulate.Auto())
                Physics.Simulate(delta);
            using (_pm_Physics2DSimulate.Auto())
                Physics2D.Simulate(delta);
        }

        /// <summary>
        /// Tries to iterate incoming or outgoing data.
        /// </summary>
        /// <param name = "incoming">True to iterate incoming.</param>
        private void TryIterateData(bool incoming)
        {
            if (incoming)
            {
                /* It's not possible for data to come in
                 * more than once per frame but there could
                 * be new data going out each tick, since
                 * movement is often based off the tick system.
                 * Because of this don't iterate incoming if
                 * it's the same frame, but the outgoing
                 * may iterate multiple times per frame due to
                 * there possibly being multiple ticks per frame. */
                int frameCount = Time.frameCount;
                if (frameCount == _lastIncomingIterationFrame)
                    return;
                _lastIncomingIterationFrame = frameCount;

                NetworkManager.TransportManager.IterateIncoming(asServer: true);
                NetworkManager.TransportManager.IterateIncoming(asServer: false);
            }
            else
            {
                NetworkManager.TransportManager.IterateOutgoing(asServer: true);
                NetworkManager.TransportManager.IterateOutgoing(asServer: false);
            }
        }

        #region Timing adjusting.
        /// <summary>
        /// Changes the adjustedTickDelta, increasing or decreasing it.
        /// </summary>
        /// <param name = "additionalMultiplier">Amount to multiply expected change by. This can be used to make larger or smaller changes.</param>
        internal void ChangeAdjustedTickDelta(bool speedUp, double additionalMultiplier = 1d)
        {
            double share = TickDelta * 0.01d * additionalMultiplier;
            if (speedUp)
                _adjustedTickDelta -= share;
            else
                _adjustedTickDelta += share;
        }

        /// <summary>
        /// Sends a TimingUpdate packet to clients.
        /// </summary>
        private void SendTimingAdjustment()
        {
            // Send every second.
            if (LocalTick % TimingTickInterval == 0)
            {
                PooledWriter writer = WriterPool.Retrieve();

                foreach (NetworkConnection item in NetworkManager.ServerManager.Clients.Values)
                {
                    if (!item.IsAuthenticated)
                        continue;

                    writer.WritePacketIdUnpacked(PacketId.TimingUpdate);
                    writer.WriteTickUnpacked(item.PacketTick.Value());
                    item.SendToClient((byte)Channel.Unreliable, writer.GetArraySegment());
                    writer.Clear();
                }

                writer.Store();

                #if DEVELOPMENT && !UNITY_SERVER
                if (_networkTrafficStatistics != null)
                {
                    // Timing updates are always a flat amount of data.
                    int written = 6 * NetworkManager.ServerManager.Clients.Count;
                    _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.TimingUpdate, string.Empty, written, gameObject: null, asServer: true);
                }
                #endif
            }
        }

        /// <summary>
        /// Called on client when server sends a timing update.
        /// </summary>
        /// <param name = "ta"></param>
        internal void ParseTimingUpdate(Reader reader)
        {
            #if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
            {
                // Timing updates are always a flat amount of data.
                int written = 6;
                _networkTrafficStatistics.AddInboundPacketIdData(PacketId.TimingUpdate, string.Empty, written, gameObject: null, asServer: false);
            }
            #endif
            uint clientTick = reader.ReadTickUnpacked();
            // Don't adjust timing on server.
            if (NetworkManager.IsServerStarted)
                return;
            /* This should never be possible since the server is sending a tick back
             * that the client previously sent. In other words, the value returned should
             * always be in the past. */
            if (LocalTick < clientTick)
                return;

            /* Use the last ordered remote tick rather than
             * lastPacketTick. This will help with out of order
             * packets where the timing update sent before
             * the remote tick but arrived after. By using ordered
             * remote tick we are comparing against however many
             * ticks really passed rather than the difference
             * between the out of order/late packet. */
            uint lastPacketTick = LastPacketTick.RemoteTick;
            // Set Tick based on difference between localTick and clientTick, added onto lastPacketTick.
            uint prevTick = Tick;
            // Added ticks for delay in reading packet.
            const uint socketReadDelay = 1;
            uint nextTick = (LocalTick - clientTick) / 2 + lastPacketTick + socketReadDelay;
            long difference = (long)nextTick - (long)prevTick;
            Tick = nextTick;

            // Maximum difference allowed before resetting values.
            const int maximumDifference = 4;
            // Difference is extreme, reset to default timings. Client probably had an issue.
            if (Mathf.Abs(difference) > maximumDifference)
            {
                _adjustedTickDelta = TickDelta;
            }
            // Otherwise adjust the delta marginally.
            else if (difference != 0)
            {
                /* A negative tickDifference indicates the client is
                 * moving too fast, while positive indicates too slow. */
                bool speedUp = difference > 0;
                ChangeAdjustedTickDelta(speedUp);
            }
        }
        #endregion

        /// <summary>
        /// Sets the TickRate to use. This value is not synchronized, it must be set on client and server independently.
        /// </summary>
        /// <param name = "value">New TickRate to use.</param>
        public void SetTickRate(ushort value)
        {
            TickRate = value;
            TickDelta = 1d / TickRate;
            _adjustedTickDelta = TickDelta;
        }

        #region UNITY_EDITOR
        private void OnValidate()
        {
            SetInitialValues();
        }
        #endregion
    }
}