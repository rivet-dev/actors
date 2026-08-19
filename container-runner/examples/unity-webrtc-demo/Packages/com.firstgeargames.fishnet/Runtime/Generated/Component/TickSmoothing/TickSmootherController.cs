#if !FISHNET_THREADED_TICKSMOOTHERS
using FishNet.Managing.Predicting;
using FishNet.Managing.Timing;
using FishNet.Object;
using GameKit.Dependencies.Utilities;
using Unity.Profiling;
using UnityEngine;

namespace FishNet.Component.Transforming.Beta
{
    /// <summary>
    /// Smoothes this object between ticks.
    /// </summary>
    /// <remarks>This can be configured to smooth over a set interval of time, or to smooth adaptively and make path corrections for prediction.</remarks>
    public class TickSmootherController : IResettable
    {
        #region Public.
        /// <summary>
        /// Logic for owner smoothing.
        /// </summary>
        public UniversalTickSmoother UniversalSmoother { get; private set; }
        #endregion

        #region Private.
        /// <summary>
        /// </summary>
        private InitializationSettings _initializationSettings = new();
        /// <summary>
        /// </summary>
        private MovementSettings _controllerMovementSettings = new();
        /// <summary>
        /// </summary>
        private MovementSettings _spectatorMovementSettings = new();
        /// <summary>
        /// True if OnDestroy has been called.
        /// </summary>
        private bool _destroyed;
        /// <summary>
        /// Cached timeManager reference.
        /// </summary>
        private TimeManager _timeManager;
        /// <summary>
        /// NetworkBehaviour which initialized this object. Value may be null when initialized for an Offline smoother.
        /// </summary>
        private NetworkBehaviour _initializingNetworkBehaviour;
        /// <summary>
        /// Transform which initialized this object.
        /// </summary>
        private Transform _graphicalTransform;
        /// <summary>
        /// True if initialized with a null NetworkBehaviour.
        /// </summary>
        private bool _initializedOffline;
        /// <summary>
        /// True if subscribed to events used for adaptiveInterpolation.
        /// </summary>
        private bool _subscribedToAdaptiveEvents;
        /// <summary>
        /// True if currently subscribed to events.
        /// </summary>
        private bool _subscribed;
        /// <summary>
        /// True if initialized.
        /// </summary>
        private bool _isInitialized;
        private static readonly ProfilerMarker _pm_OnUpdate = new("TickSmootherController.TimeManager_OnUpdate()");
        private static readonly ProfilerMarker _pm_OnPreTick = new("TickSmootherController.TimeManager_OnPreTick()");
        private static readonly ProfilerMarker _pm_OnPostTick = new("TickSmootherController.TimeManager_OnPostTick()");
        #endregion

        public void Initialize(InitializationSettings initializationSettings, MovementSettings controllerSettings, MovementSettings spectatorSettings)
        {
            _initializingNetworkBehaviour = initializationSettings.InitializingNetworkBehaviour;
            _graphicalTransform = initializationSettings.GraphicalTransform;

            _initializationSettings = initializationSettings;
            _controllerMovementSettings = controllerSettings;
            _spectatorMovementSettings = spectatorSettings;

            _initializedOffline = initializationSettings.InitializingNetworkBehaviour == null;

            _isInitialized = true;
        }

        public void OnDestroy()
        {
            ChangeSubscriptions(false);
            StoreSmoother();
            _destroyed = true;
            _isInitialized = false;
        }

        public void StartSmoother()
        {
            if (!_isInitialized)
                return;

            bool canStart = _initializedOffline ? StartOffline() : StartOnline();

            if (!canStart)
                return;

            RetrieveSmoothers();

            UniversalSmoother.Initialize(_initializationSettings, _controllerMovementSettings, _spectatorMovementSettings);

            UniversalSmoother.StartSmoother();

            bool StartOnline()
            {
                NetworkBehaviour nb = _initializingNetworkBehaviour;

                SetTimeManager(nb.TimeManager);

                return true;
            }

            bool StartOffline()
            {
                if (_timeManager == null)
                    return false;

                return true;
            }
        }

        public void StopSmoother()
        {
            ChangeSubscriptions(subscribe: false);

            if (!_initializedOffline)
                StopOnline();

            if (UniversalSmoother != null)
                UniversalSmoother.StopSmoother();

            void StopOnline()
            {
                SetTimeManager(tm: null);
            }

            // Intentionally left blank.
            // void StopOffline() { }
        }

        public void TimeManager_OnUpdate()
        {
            using (_pm_OnUpdate.Auto())
            {
                UniversalSmoother.OnUpdate(Time.deltaTime);
            }
        }

        public void TimeManager_OnPreTick()
        {
            using (_pm_OnPreTick.Auto())
            {
                UniversalSmoother.OnPreTick();
            }
        }

        /// <summary>
        /// Called after a tick completes.
        /// </summary>
        public void TimeManager_OnPostTick()
        {
            using (_pm_OnPostTick.Auto())
            {
                if (_timeManager != null)
                    UniversalSmoother.OnPostTick(_timeManager.LocalTick);
            }
        }

        private void PredictionManager_OnPostReplicateReplay(uint clientTick, uint serverTick)
        {
            UniversalSmoother.OnPostReplicateReplay(clientTick);
        }

        private void TimeManager_OnRoundTripTimeUpdated(long rttMs)
        {
            UniversalSmoother.UpdateRealtimeInterpolation();
        }

        /// <summary>
        /// Stores smoothers if they have value.
        /// </summary>
        private void StoreSmoother()
        {
            if (UniversalSmoother == null)
                return;

            ResettableObjectCaches<UniversalTickSmoother>.Store(UniversalSmoother);
            UniversalSmoother = null;
        }

        /// <summary>
        /// Stores current smoothers and retrieves new ones.
        /// </summary>
        private void RetrieveSmoothers()
        {
            StoreSmoother();
            UniversalSmoother = ResettableObjectCaches<UniversalTickSmoother>.Retrieve();
        }

        // /// <summary>
        // /// Sets a target transform to follow.
        // /// </summary>
        // public void SetTargetTransform(Transform value)
        // {
        //     Transform currentTargetTransform = _initializationSettings.TargetTransform;
        //
        //     if (value == currentTargetTransform)
        //         return;
        //
        //     bool clientStartCalled = (_initializedOffline && _timeManager != null) || (_initializingNetworkBehaviour != null && _initializingNetworkBehaviour.OnStartClientCalled);
        //
        //     bool previousTargetTransformIsValid = (currentTargetTransform != null);
        //
        //     // If target is different and old is not null then reset.
        //     if (previousTargetTransformIsValid && clientStartCalled)
        //         OnStopClient();
        //
        //     _initializationSettings.TargetTransform = value;
        //     if (previousTargetTransformIsValid && clientStartCalled)
        //         OnStartClient();
        // }

        /// <summary>
        /// Sets a new PredictionManager to use.
        /// </summary>
        public void SetTimeManager(TimeManager tm)
        {
            if (tm == _timeManager)
                return;

            // Unsub from current.
            ChangeSubscriptions(false);
            //Sub to newest.
            _timeManager = tm;
            ChangeSubscriptions(true);
        }

        /// <summary>
        /// Changes the subscription to the TimeManager.
        /// </summary>
        private void ChangeSubscriptions(bool subscribe)
        {
            if (_destroyed)
                return;
            TimeManager tm = _timeManager;
            if (tm == null)
                return;

            if (subscribe == _subscribed)
                return;
            _subscribed = subscribe;

            bool adaptiveIsOff = _controllerMovementSettings.AdaptiveInterpolationValue == AdaptiveInterpolationType.Off && _spectatorMovementSettings.AdaptiveInterpolationValue == AdaptiveInterpolationType.Off;

            if (subscribe)
            {
                tm.OnUpdate += TimeManager_OnUpdate;
                tm.OnPreTick += TimeManager_OnPreTick;
                tm.OnPostTick += TimeManager_OnPostTick;

                if (!adaptiveIsOff)
                {
                    tm.OnRoundTripTimeUpdated += TimeManager_OnRoundTripTimeUpdated;
                    PredictionManager pm = tm.NetworkManager.PredictionManager;
                    pm.OnPostReplicateReplay += PredictionManager_OnPostReplicateReplay;
                    _subscribedToAdaptiveEvents = true;
                }
            }
            else
            {
                tm.OnUpdate -= TimeManager_OnUpdate;
                tm.OnPreTick -= TimeManager_OnPreTick;
                tm.OnPostTick -= TimeManager_OnPostTick;

                if (_subscribedToAdaptiveEvents)
                {
                    tm.OnRoundTripTimeUpdated -= TimeManager_OnRoundTripTimeUpdated;
                    PredictionManager pm = tm.NetworkManager.PredictionManager;
                    pm.OnPostReplicateReplay -= PredictionManager_OnPostReplicateReplay;
                }
            }
        }

        public void ResetState()
        {
            _initializationSettings = default;
            _controllerMovementSettings = default;
            _spectatorMovementSettings = default;

            _destroyed = false;
            _timeManager = null;
            _initializingNetworkBehaviour = null;
            _graphicalTransform = null;

            _subscribed = false;
            _subscribedToAdaptiveEvents = false;

            _isInitialized = false;
        }

        public void InitializeState() { }
    }
}
#endif