#if FISHNET_THREADED_TICKSMOOTHERS
using System.Collections.Generic;
using FishNet.Managing;
using FishNet.Managing.Predicting;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Object.Prediction;
using FishNet.Transporting;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using GameKit.Dependencies.Utilities.Types;
using Unity.Collections;
using Unity.Jobs;
using Unity.Jobs.LowLevel.Unsafe;
using Unity.Profiling;
using UnityEngine;
using UnityEngine.Jobs;

namespace FishNet.Component.Transforming.Beta
{
    public partial class TickSmoothingManager : MonoBehaviour
    {
        #region Private.
        
        #region Private Profiler Markers
        private static readonly ProfilerMarker _pm_ClientManager_OnClientConnectionState            = new("TickSmoothingManager.Client_OnClientConnectionState");
        private static readonly ProfilerMarker _pm_OnUpdate                                         = new("TickSmoothingManager.TimeManager_OnUpdate()");
        private static readonly ProfilerMarker _pm_OnPreTick                                        = new("TickSmoothingManager.TimeManager_OnPreTick()");
        private static readonly ProfilerMarker _pm_OnPostTick                                       = new("TickSmoothingManager.TimeManager_OnPostTick()");
        private static readonly ProfilerMarker _pm_Prediction_OnPostReplicateReplay                 = new("TickSmoothingManager.Prediction_OnPostReplicateReplay()");
        private static readonly ProfilerMarker _pm_TimeManager_OnRoundTripTimeUpdated               = new("TickSmoothingManager.TimeManager_OnRoundTripTimeUpdated()");
        private static readonly ProfilerMarker _pm_MoveToTarget                                     = new("TickSmoothingManager.MoveToTarget()");
        private static readonly ProfilerMarker _pm_ScheduleUpdateRealtimeInterpolation              = new("TickSmoothingManager.ScheduleUpdateRealtimeInterpolation()");
        private static readonly ProfilerMarker _pm_ScheduleDiscardExcessiveTransformPropertiesQueue = new("TickSmoothingManager.ScheduleDiscardExcessiveTransformPropertiesQueue()");
        private static readonly ProfilerMarker _pm_ScheduleSetMoveRates                             = new("TickSmoothingManager.ScheduleSetMoveRates()");
        private static readonly ProfilerMarker _pm_ScheduleSetMovementMultiplier                    = new("TickSmoothingManager.ScheduleSetMovementMultiplier()");
        private static readonly ProfilerMarker _pm_ScheduleAddTransformProperties                   = new("TickSmoothingManager.ScheduleAddTransformProperties()");
        private static readonly ProfilerMarker _pm_ScheduleClearTransformPropertiesQueue            = new("TickSmoothingManager.ScheduleClearTransformPropertiesQueue()");
        private static readonly ProfilerMarker _pm_ScheduleModifyTransformProperties                = new("TickSmoothingManager.ScheduleModifyTransformProperties()");
        private static readonly ProfilerMarker _pm_ScheduleSnapNonSmoothedProperties                = new("TickSmoothingManager.ScheduleSnapNonSmoothedProperties()");
        private static readonly ProfilerMarker _pm_ScheduleTeleport                                 = new("TickSmoothingManager.ScheduleTeleport()");
        private static readonly ProfilerMarker _pm_Register                                         = new("TickSmoothingManager.Register()");
        private static readonly ProfilerMarker _pm_Unregister                                       = new("TickSmoothingManager.Unregister()");
        #endregion
        
        #region Const.
        /// <summary>
        /// Maximum allowed entries.
        /// </summary>
        private const int MAXIMUM_QUEUED = 256;
        /// <summary>
        /// Maximum allowed entries to be queued over the interpolation amount.
        /// </summary>
        private const int REQUIRED_QUEUED_OVER_INTERPOLATION = 3;
        #endregion
        
        /// <summary>
        /// NetworkManager on the same object as this script.
        /// </summary>
        private NetworkManager _networkManager;
        /// <summary>
        /// TimeManager on the same object as this script.
        /// </summary>
        private TimeManager _timeManager;
        /// <summary>
        /// PredictionManager on the same object as this script.
        /// </summary>
        private PredictionManager _predictionManager;
        
        /// <summary>
        /// TrackerTransformsPool.
        /// </summary>
        private readonly Stack<Transform> _trackerTransformsPool = new();
        /// <summary>
        /// TrackerTransformsPoolHolder.
        /// </summary>
        private Transform _trackerTransformsPoolHolder;
        
        /// <summary>
        /// TickSmootherController to index lookup.
        /// </summary>
        private readonly Dictionary<TickSmootherController, int> _lookup = new();
        /// <summary>
        /// Index to TickSmootherController and InitializationSettings lookup. 
        /// </summary>
        private readonly List<TickSmootherController> _indexToSmoother = new();
        /// <summary>
        /// Index to TickSmootherController and NetworkBehaviours lookup. 
        /// </summary>
        private readonly List<NetworkBehaviour> _indexToNetworkBehaviour = new();
        /// <summary>
        /// Index to TickSmootherController and redictionNetworkTransform lookup. 
        /// </summary>
        private readonly List<NetworkTransform> _indexToPredictionNetworkTransform = new();
        
        /// <summary>
        /// Index to MoveRate lookup.
        /// How quickly to move towards goal values.
        /// </summary>
        private NativeList<MoveRates> _moveRates;
        /// <summary>
        /// Index to Owner MovementSettings lookup.
        /// Settings to use for owners.
        /// </summary>
        private NativeList<MovementSettings> _ownerSettings;
        /// <summary>
        /// Index to Spectator MovementSettings lookup.
        /// Settings to use for spectators.
        /// </summary>
        private NativeList<MovementSettings> _spectatorSettings;
        
        /// <summary>
        /// Index to PreTickedMask lookup.
        /// True if a pretick occurred since last postTick.
        /// </summary>
        private NativeList<byte> _preTickedMask;
        /// <summary>
        /// Index to MoveImmediatelyMask lookup.
        /// True to begin moving soon as movement data becomes available. Movement will ease in until at interpolation value. False to prevent movement until movement data count meet interpolation.
        /// </summary>
        private NativeList<byte> _moveImmediatelyMask;
        /// <summary>
        /// Index to CanSmoothMask lookup.
        /// Returns if prediction can be used on this rigidbody.
        /// </summary>
        private NativeList<byte> _canSmoothMask;
        /// <summary>
        /// Index to UseOwnerSettingsMask lookup.
        /// True if to smooth using owner settings, false for spectator settings.
        /// This is only used for performance gains.
        /// </summary>
        private NativeList<byte> _useOwnerSettingsMask;
        /// <summary>
        /// Index to ObjectReconcilingMask lookup.
        /// </summary>
        private NativeList<byte> _objectReconcilingMask;
        /// <summary>
        /// Index to DetachOnStartMask lookup.
        /// True if to detach on smoothing start.
        /// </summary>
        private NativeList<byte> _detachOnStartMask;
        /// <summary>
        /// Index to AttachOnStopMask lookup.
        /// True if to attach on smoothing stop.
        /// </summary>
        private NativeList<byte> _attachOnStopMask;
        /// <summary>
        /// Index to IsMoving lookup.
        /// True if moving has started and has not been stopped.
        /// </summary>
        private NativeList<byte> _isMoving;
        /// <summary>
        /// Index to TeleportedTick lookup.
        /// Last tick this was teleported on.
        /// </summary>
        private NativeList<uint> _teleportedTick;
        /// <summary>
        /// Index to RealTimeInterpolation lookup.
        /// Current interpolation value, be it a flat value or adaptive.
        /// </summary>
        private NativeList<byte> _realTimeInterpolations;
        /// <summary>
        /// Index to MovementMultiplier lookup.
        /// Value to multiply movement by. This is used to reduce or increase the rate the movement buffer is consumed.
        /// </summary>
        private NativeList<float> _movementMultipliers;
        
        /// <summary>
        /// Index to TransformProperties lookup.
        /// TransformProperties to move towards
        /// </summary>
        private StripedRingQueue<TickTransformProperties> _transformProperties;
        /// <summary>
        /// Index to PreTick Graphic TransformProperties Snapshot lookup.
        /// World values of the graphical after it's been aligned to initialized values in PreTick.
        /// </summary>
        private NativeList<TransformProperties> _preTickGraphicSnapshot;
        /// <summary>
        /// Index to PreTick Tracker TransformProperties Snapshot lookup.
        /// World values of the graphical after it's been aligned to initialized values in PreTick.
        /// </summary>
        private NativeList<TransformProperties> _postTickTrackerSnapshot;
        /// <summary>
        /// Index to Temp Target TransformProperties Snapshot lookup.
        /// </summary>
        private NativeList<TransformProperties> _tempTargetSnapshot;
        /// <summary>
        /// Index to OutSnapGraphicWorld lookup.
        /// </summary>
        private NativeList<TransformProperties> _outSnapGraphicWorld;
        /// <summary>
        /// Index to OutEnqueueTrackerWorld lookup.
        /// </summary>
        private NativeList<TransformProperties> _outEnqueueTrackerWorld;
        /// <summary>
        /// Index to QueuedTrackerProperties lookup.
        /// Properties for the tracker which are queued to be set when the tracker is setup.
        /// </summary>
        private NativeList<NullableTransformProperties> _queuedTrackerProperties;
        
        /// <summary>
        /// Index to MoveToTargetPayloads lookup.
        /// </summary>
        private NativeList<MoveToTargetPayload> _moveToTargetPayloads;
        /// <summary>
        /// Index to UpdateRealtimeInterpolationPayloads lookup.
        /// </summary>
        private NativeList<UpdateRealtimeInterpolationPayload> _updateRealtimeInterpolationPayloads;
        /// <summary>
        /// Index to DiscardExcessiveTransformPropertiesQueuePayloads lookup.
        /// </summary>
        private NativeList<DiscardExcessiveTransformPropertiesQueuePayload> _discardExcessivePayloads;
        /// <summary>
        /// Index to SetMoveRatesPayloads lookup.
        /// </summary>
        private NativeList<SetMoveRatesPayload> _setMoveRatesPayloads;
        /// <summary>
        /// Index to SetMovementMultiplierPayloads lookup.
        /// </summary>
        private NativeList<SetMovementMultiplierPayload> _setMovementMultiplierPayloads;
        /// <summary>
        /// Index to AddTransformPropertiesPayloads lookup.
        /// </summary>
        private NativeList<AddTransformPropertiesPayload> _addTransformPropertiesPayloads;
        /// <summary>
        /// Index to ClearTransformPropertiesQueuePayloads lookup.
        /// </summary>
        private NativeList<ClearTransformPropertiesQueuePayload> _clearTransformPropertiesQueuePayloads;
        /// <summary>
        /// Index to ModifyTransformPropertiesPayloads lookup.
        /// </summary>
        private NativeList<ModifyTransformPropertiesPayload> _modifyTransformPropertiesPayloads;
        /// <summary>
        /// Index to SnapNonSmoothedPropertiesPayloads lookup.
        /// </summary>
        private NativeList<SnapNonSmoothedPropertiesPayload> _snapNonSmoothedPropertiesPayloads;
        /// <summary>
        /// Index to TeleportPayloads lookup.
        /// </summary>
        private NativeList<TeleportPayload> _teleportPayloads;
        
        /// <summary>
        /// Target objects TransformAccessArray.
        /// Transform the graphics should follow.
        /// </summary>
        private TransformAccessArray _targetTaa;
        /// <summary>
        /// Graphical objects TransformAccessArray.
        /// Cached value of the object to smooth.
        /// </summary>
        private TransformAccessArray _graphicalTaa;
        /// <summary>
        /// Tracker objects TransformAccessArray.
        /// Empty gameObject containing a transform which has properties checked after each simulation.
        /// If the graphical starts off as nested of targetTransform then this object is created where the graphical object is.
        /// Otherwise, this object is placed directly beneath targetTransform.
        /// </summary>
        private TransformAccessArray _trackerTaa;
        
        /// <summary>
        /// Subscription to callbacks state.
        /// </summary>
        private bool _subscribed;
        #endregion

        /// <summary>
        /// Initialize once from NetworkManager (pattern mirrors RollbackManager).
        /// </summary>
        internal void InitializeOnce_Internal(NetworkManager manager)
        {
            _networkManager = manager;
            _timeManager = manager.TimeManager;
            _predictionManager = manager.PredictionManager;

            if (!_trackerTransformsPoolHolder)
            {
                _trackerTransformsPoolHolder = new GameObject("Tracker Transforms Pool Holder").transform;
                DontDestroyOnLoad(_trackerTransformsPoolHolder.gameObject);
            }
            
            if (!_moveRates.IsCreated) _moveRates                             = new NativeList<MoveRates>(64, Allocator.Persistent);
            if (!_ownerSettings.IsCreated) _ownerSettings                     = new NativeList<MovementSettings>(64, Allocator.Persistent);
            if (!_spectatorSettings.IsCreated) _spectatorSettings             = new NativeList<MovementSettings>(64, Allocator.Persistent);
            
            if (!_preTickedMask.IsCreated) _preTickedMask                     = new NativeList<byte>(64, Allocator.Persistent);
            if (!_canSmoothMask.IsCreated) _canSmoothMask                     = new NativeList<byte>(64, Allocator.Persistent);
            if (!_useOwnerSettingsMask.IsCreated) _useOwnerSettingsMask       = new NativeList<byte>(64, Allocator.Persistent);
            if (!_objectReconcilingMask.IsCreated) _objectReconcilingMask     = new NativeList<byte>(64, Allocator.Persistent);
            if (!_detachOnStartMask.IsCreated) _detachOnStartMask             = new NativeList<byte>(64, Allocator.Persistent);
            if (!_attachOnStopMask.IsCreated) _attachOnStopMask               = new NativeList<byte>(64, Allocator.Persistent);
            if (!_moveImmediatelyMask.IsCreated) _moveImmediatelyMask         = new NativeList<byte>(64, Allocator.Persistent);
            if (!_isMoving.IsCreated) _isMoving                               = new NativeList<byte>(64, Allocator.Persistent);
            if (!_teleportedTick.IsCreated) _teleportedTick                   = new NativeList<uint>(64, Allocator.Persistent);
            if (!_realTimeInterpolations.IsCreated) _realTimeInterpolations   = new NativeList<byte>(64, Allocator.Persistent);
            if (!_movementMultipliers.IsCreated) _movementMultipliers         = new NativeList<float>(64, Allocator.Persistent);
            
            if (!_transformProperties.IsCreated) _transformProperties         = new StripedRingQueue<TickTransformProperties>(64, MAXIMUM_QUEUED, Allocator.Persistent);
            if (!_preTickGraphicSnapshot.IsCreated) _preTickGraphicSnapshot   = new NativeList<TransformProperties>(64, Allocator.Persistent);
            if (!_postTickTrackerSnapshot.IsCreated) _postTickTrackerSnapshot = new NativeList<TransformProperties>(64, Allocator.Persistent);
            if (!_tempTargetSnapshot.IsCreated) _tempTargetSnapshot           = new NativeList<TransformProperties>(64, Allocator.Persistent);
            if (!_outSnapGraphicWorld.IsCreated) _outSnapGraphicWorld         = new NativeList<TransformProperties>(64, Allocator.Persistent);
            if (!_outEnqueueTrackerWorld.IsCreated) _outEnqueueTrackerWorld   = new NativeList<TransformProperties>(64, Allocator.Persistent);
            if (!_queuedTrackerProperties.IsCreated) _queuedTrackerProperties = new NativeList<NullableTransformProperties>(64, Allocator.Persistent);

            if (!_moveToTargetPayloads.IsCreated) _moveToTargetPayloads                                   = new NativeList<MoveToTargetPayload>(64, Allocator.Persistent);
            if (!_updateRealtimeInterpolationPayloads.IsCreated) _updateRealtimeInterpolationPayloads     = new NativeList<UpdateRealtimeInterpolationPayload>(64, Allocator.Persistent);
            if (!_discardExcessivePayloads.IsCreated) _discardExcessivePayloads                           = new NativeList<DiscardExcessiveTransformPropertiesQueuePayload>(64, Allocator.Persistent);
            if (!_setMoveRatesPayloads.IsCreated) _setMoveRatesPayloads                                   = new NativeList<SetMoveRatesPayload>(64, Allocator.Persistent);
            if (!_setMovementMultiplierPayloads.IsCreated) _setMovementMultiplierPayloads                 = new NativeList<SetMovementMultiplierPayload>(64, Allocator.Persistent);
            if (!_addTransformPropertiesPayloads.IsCreated) _addTransformPropertiesPayloads               = new NativeList<AddTransformPropertiesPayload>(64, Allocator.Persistent);
            if (!_clearTransformPropertiesQueuePayloads.IsCreated) _clearTransformPropertiesQueuePayloads = new NativeList<ClearTransformPropertiesQueuePayload>(64, Allocator.Persistent);
            if (!_modifyTransformPropertiesPayloads.IsCreated) _modifyTransformPropertiesPayloads         = new NativeList<ModifyTransformPropertiesPayload>(64, Allocator.Persistent);
            if (!_snapNonSmoothedPropertiesPayloads.IsCreated) _snapNonSmoothedPropertiesPayloads         = new NativeList<SnapNonSmoothedPropertiesPayload>(64, Allocator.Persistent);
            if (!_teleportPayloads.IsCreated) _teleportPayloads                                           = new NativeList<TeleportPayload>(64, Allocator.Persistent);
            
            if (!_targetTaa.isCreated) _targetTaa       = new TransformAccessArray(64);
            if (!_graphicalTaa.isCreated) _graphicalTaa = new TransformAccessArray(64);
            if (!_trackerTaa.isCreated) _trackerTaa     = new TransformAccessArray(64);
            
            // Subscribe to client connection state to (un)hook timing/prediction.
            _networkManager.ClientManager.OnClientConnectionState += ClientManager_OnClientConnectionState;
            if (_networkManager.ClientManager.Started) ChangeSubscriptions(true);
        }

        private void OnDestroy()
        {
            ChangeSubscriptions(false);

            if (_networkManager != null)
            {
                _networkManager.ClientManager.OnClientConnectionState -= ClientManager_OnClientConnectionState;
            }
            
            while (_trackerTransformsPool.TryPop(out Transform trackerTransform))
                if (trackerTransform && trackerTransform.gameObject) Destroy(trackerTransform.gameObject);

            for (int i = 0; i < _indexToSmoother.Count; i++)
            {
                Transform trackerTransform = _trackerTaa[i];
                if (trackerTransform && trackerTransform.gameObject) Destroy(trackerTransform.gameObject);
            }
           
            if (_moveRates.IsCreated)               _moveRates.Dispose();
            if (_ownerSettings.IsCreated)           _ownerSettings.Dispose();
            if (_spectatorSettings.IsCreated)       _spectatorSettings.Dispose();
            
            if (_preTickedMask.IsCreated)           _preTickedMask.Dispose();
            if (_canSmoothMask.IsCreated)           _canSmoothMask.Dispose();
            if (_useOwnerSettingsMask.IsCreated)    _useOwnerSettingsMask.Dispose();
            if (_objectReconcilingMask.IsCreated)   _objectReconcilingMask.Dispose();
            if (_detachOnStartMask.IsCreated)       _detachOnStartMask.Dispose();
            if (_attachOnStopMask.IsCreated)        _attachOnStopMask.Dispose();
            if (_moveImmediatelyMask.IsCreated)     _moveImmediatelyMask.Dispose();
            if (_isMoving.IsCreated)                _isMoving.Dispose();
            if (_teleportedTick.IsCreated)          _teleportedTick.Dispose();
            if (_realTimeInterpolations.IsCreated)  _realTimeInterpolations.Dispose();
            if (_movementMultipliers.IsCreated)     _movementMultipliers.Dispose();
            
            if (_transformProperties.IsCreated)     _transformProperties.Dispose();
            if (_preTickGraphicSnapshot.IsCreated)  _preTickGraphicSnapshot.Dispose();
            if (_postTickTrackerSnapshot.IsCreated) _postTickTrackerSnapshot.Dispose();
            if (_tempTargetSnapshot.IsCreated)      _tempTargetSnapshot.Dispose();
            if (_outSnapGraphicWorld.IsCreated)     _outSnapGraphicWorld.Dispose();
            if (_outEnqueueTrackerWorld.IsCreated)  _outEnqueueTrackerWorld.Dispose();
            if (_queuedTrackerProperties.IsCreated) _queuedTrackerProperties.Dispose();

            if (_moveToTargetPayloads.IsCreated)                  _moveToTargetPayloads.Dispose();
            if (_updateRealtimeInterpolationPayloads.IsCreated)   _updateRealtimeInterpolationPayloads.Dispose();
            if (_discardExcessivePayloads.IsCreated)              _discardExcessivePayloads.Dispose();
            if (_setMoveRatesPayloads.IsCreated)                  _setMoveRatesPayloads.Dispose();
            if (_setMovementMultiplierPayloads.IsCreated)         _setMovementMultiplierPayloads.Dispose();
            if (_addTransformPropertiesPayloads.IsCreated)        _addTransformPropertiesPayloads.Dispose();
            if (_clearTransformPropertiesQueuePayloads.IsCreated) _clearTransformPropertiesQueuePayloads.Dispose();
            if (_modifyTransformPropertiesPayloads.IsCreated)     _modifyTransformPropertiesPayloads.Dispose();
            if (_snapNonSmoothedPropertiesPayloads.IsCreated)     _snapNonSmoothedPropertiesPayloads.Dispose();
            if (_teleportPayloads.IsCreated)                      _teleportPayloads.Dispose();
            
            if (_targetTaa.isCreated)    _targetTaa.Dispose();
            if (_graphicalTaa.isCreated) _graphicalTaa.Dispose();
            if (_trackerTaa.isCreated)   _trackerTaa.Dispose();
            
            _indexToNetworkBehaviour.Clear();
            _indexToPredictionNetworkTransform.Clear();
            _indexToSmoother.Clear();
            _lookup.Clear();

            _networkManager = null;
            _timeManager = null;
            _predictionManager = null;
        }
        
        /// <summary>
        /// Register a TickSmootherController with associated settings.
        /// </summary>
        public void Register(TickSmootherController smoother, InitializationSettings initializationSettings,
            MovementSettings ownerSettings, MovementSettings spectatorSettings)
        {
            using (_pm_Register.Auto())
            {
                if (smoother == null)
                    return;
                
                if (!TransformsAreValid(initializationSettings.GraphicalTransform, initializationSettings.TargetTransform))
                    return;

                /* Unset scale smoothing if not detaching. This is to prevent
                 * the scale from changing with the parent if nested, as that
                 * would result in the scale being modified twice, once on the parent
                 * and once on the graphical. Thanks deo_wh for find! */
                if (!initializationSettings.DetachOnStart)
                {
                    ownerSettings.SmoothedProperties &= ~TransformPropertiesFlag.Scale;
                    spectatorSettings.SmoothedProperties &= ~TransformPropertiesFlag.Scale;
                }
                
                if (_lookup.TryGetValue(smoother, out int index))
                {
                    _ownerSettings[index] = ownerSettings;
                    _spectatorSettings[index] = spectatorSettings;
                    return;
                }
                index = _indexToSmoother.Count;

                _lookup[smoother] = index;
                _indexToSmoother.Add(smoother);
                _indexToNetworkBehaviour.Add(initializationSettings.InitializingNetworkBehaviour);
                _indexToPredictionNetworkTransform.Add(
                    initializationSettings.FavorPredictionNetworkTransform &&
                    initializationSettings.InitializingNetworkBehaviour != null &&
                    initializationSettings.InitializingNetworkBehaviour.NetworkObject != null &&
                    initializationSettings.InitializingNetworkBehaviour.NetworkObject.IsRigidbodyPredictionType
                    ? initializationSettings.InitializingNetworkBehaviour.NetworkObject.PredictionNetworkTransform
                    : null
                    );

                _moveRates.Add(new MoveRates(MoveRates.UNSET_VALUE));
                _ownerSettings.Add(ownerSettings);
                _spectatorSettings.Add(spectatorSettings);
                
                _preTickedMask.Add(0);
                _canSmoothMask.Add(
                    (byte)(initializationSettings.GraphicalTransform != null && 
                    _networkManager.IsClientStarted ? 1 : 0));
                _useOwnerSettingsMask.Add(
                    (byte)(initializationSettings.InitializingNetworkBehaviour == null ||
                    initializationSettings.InitializingNetworkBehaviour.IsOwner ||
                    !initializationSettings.InitializingNetworkBehaviour.Owner.IsValid ? 1 : 0));
                _objectReconcilingMask.Add(
                    (byte)(initializationSettings.InitializingNetworkBehaviour == null ||
                        initializationSettings.InitializingNetworkBehaviour.NetworkObject == null ||
                        initializationSettings.InitializingNetworkBehaviour.NetworkObject.IsObjectReconciling ? 1 : 0));
                _detachOnStartMask.Add(
                    (byte)(initializationSettings.DetachOnStart ? 1 : 0));
                _attachOnStopMask.Add(
                    (byte)(initializationSettings.AttachOnStop ? 1 : 0));
                _moveImmediatelyMask.Add(
                    (byte)(initializationSettings.MoveImmediately ? 1 : 0));
                
                _isMoving.Add(default);
                _teleportedTick.Add(TimeManager.UNSET_TICK);
                _realTimeInterpolations.Add(default);
                _movementMultipliers.Add(default);
                
                _transformProperties.AddQueue();
                _preTickGraphicSnapshot.Add(default);
                _postTickTrackerSnapshot.Add(default);
                _tempTargetSnapshot.Add(default);
                _outSnapGraphicWorld.Add(default);
                _outEnqueueTrackerWorld.Add(default);
                _queuedTrackerProperties.Add(new NullableTransformProperties(false, default));
                
                _moveToTargetPayloads.Add(new MoveToTargetPayload(0, default));
                _updateRealtimeInterpolationPayloads.Add(new UpdateRealtimeInterpolationPayload(0));
                _discardExcessivePayloads.Add(new DiscardExcessiveTransformPropertiesQueuePayload(0));
                _setMoveRatesPayloads.Add(new SetMoveRatesPayload(0, default));
                _setMovementMultiplierPayloads.Add(new SetMovementMultiplierPayload(0));
                _addTransformPropertiesPayloads.Add(new AddTransformPropertiesPayload(0, default));
                _clearTransformPropertiesQueuePayloads.Add(new ClearTransformPropertiesQueuePayload(0));
                _modifyTransformPropertiesPayloads.Add(new ModifyTransformPropertiesPayload(0, default, default));
                _snapNonSmoothedPropertiesPayloads.Add(new SnapNonSmoothedPropertiesPayload(0, default));
                _teleportPayloads.Add(new TeleportPayload(0));
                
                Transform targetTransform = initializationSettings.TargetTransform;
                Transform graphicalTransform = initializationSettings.GraphicalTransform;
                if (!_trackerTransformsPool.TryPop(out Transform trackerTransform)) 
                    trackerTransform = new GameObject().transform;
                ProcessTransformsOnStart(trackerTransform, targetTransform, graphicalTransform, initializationSettings.DetachOnStart);

                _targetTaa.Add(targetTransform);
                _graphicalTaa.Add(graphicalTransform);
                _trackerTaa.Add(trackerTransform);
                
                //Use set method as it has sanity checks.
                SetInterpolationValue(smoother, ownerSettings.InterpolationValue, forOwnerOrOfflineSmoother: true, unsetAdaptiveInterpolation: false);
                SetInterpolationValue(smoother, spectatorSettings.InterpolationValue, forOwnerOrOfflineSmoother: false, unsetAdaptiveInterpolation: false);

                SetAdaptiveInterpolation(smoother, ownerSettings.AdaptiveInterpolationValue, forOwnerOrOfflineSmoother: true);
                SetAdaptiveInterpolation(smoother, spectatorSettings.AdaptiveInterpolationValue, forOwnerOrOfflineSmoother: false);
            }
        }

        /// <summary>
        /// Unregister a TickSmootherController.
        /// </summary>
        public void Unregister(TickSmootherController smoother)
        {
            using (_pm_Unregister.Auto())
            {
                if (smoother == null || !_lookup.TryGetValue(smoother, out int index))
                    return;

                bool isDetachOnStart = _detachOnStartMask[index] != 0;
                bool isAttachOnStop = _attachOnStopMask[index] != 0;
                Transform targetTransform = _targetTaa[index];
                Transform graphicalTransform = _graphicalTaa[index];
                Transform trackerTransform = _trackerTaa[index];
                ProcessTransformsOnStop(trackerTransform, targetTransform, graphicalTransform, isDetachOnStart, isAttachOnStop);
                if (trackerTransform)
                {
                    _trackerTransformsPool.Push(trackerTransform);
                    trackerTransform.SetParent(_trackerTransformsPoolHolder);
                }
                
                int last = _indexToSmoother.Count - 1;
                if (index != last)
                {
                    var movedSmoother = _indexToSmoother[last];
                    _indexToSmoother[index] = movedSmoother;
                    _lookup[movedSmoother] = index;
                    var movedNetworkBehaviour = _indexToNetworkBehaviour[last];
                    _indexToNetworkBehaviour[index] = movedNetworkBehaviour;
                    var movedPredictionNetworkTransform = _indexToPredictionNetworkTransform[last];
                    _indexToPredictionNetworkTransform[index] = movedPredictionNetworkTransform;
                }

                _indexToNetworkBehaviour.RemoveAt(last);
                _indexToPredictionNetworkTransform.RemoveAt(last);
                _indexToSmoother.RemoveAt(last);
                _lookup.Remove(smoother);

                _moveRates.RemoveAtSwapBack(index);
                _ownerSettings.RemoveAtSwapBack(index);
                _spectatorSettings.RemoveAtSwapBack(index);
                
                _preTickedMask.RemoveAtSwapBack(index);
                _canSmoothMask.RemoveAtSwapBack(index);
                _useOwnerSettingsMask.RemoveAtSwapBack(index);
                _objectReconcilingMask.RemoveAtSwapBack(index);
                _detachOnStartMask.RemoveAtSwapBack(index);
                _attachOnStopMask.RemoveAtSwapBack(index);
                _moveImmediatelyMask.RemoveAtSwapBack(index);
                
                _isMoving.RemoveAtSwapBack(index);
                _teleportedTick.RemoveAtSwapBack(index);
                _realTimeInterpolations.RemoveAtSwapBack(index);
                _movementMultipliers.RemoveAtSwapBack(index);
                
                _transformProperties.RemoveQueueAtSwapBack(index);
                _preTickGraphicSnapshot.RemoveAtSwapBack(index);
                _postTickTrackerSnapshot.RemoveAtSwapBack(index);
                _tempTargetSnapshot.RemoveAtSwapBack(index);
                _outSnapGraphicWorld.RemoveAtSwapBack(index);
                _outEnqueueTrackerWorld.RemoveAtSwapBack(index);
                _queuedTrackerProperties.RemoveAtSwapBack(index);
                
                _targetTaa.RemoveAtSwapBack(index);
                _graphicalTaa.RemoveAtSwapBack(index);
                _trackerTaa.RemoveAtSwapBack(index);
                
                _moveToTargetPayloads.RemoveAtSwapBack(index);
                _updateRealtimeInterpolationPayloads.RemoveAtSwapBack(index);
                _discardExcessivePayloads.RemoveAtSwapBack(index);
                _setMoveRatesPayloads.RemoveAtSwapBack(index);
                _setMovementMultiplierPayloads.RemoveAtSwapBack(index);
                _addTransformPropertiesPayloads.RemoveAtSwapBack(index);
                _clearTransformPropertiesQueuePayloads.RemoveAtSwapBack(index);
                _modifyTransformPropertiesPayloads.RemoveAtSwapBack(index);
                _snapNonSmoothedPropertiesPayloads.RemoveAtSwapBack(index);
                _teleportPayloads.RemoveAtSwapBack(index);
            }
        }
        
        /// <summary>
        /// Returns if configured transforms are valid.
        /// </summary>
        /// <returns></returns>
        private static bool TransformsAreValid(Transform graphicalTransform, Transform targetTransform)
        {
            if (graphicalTransform == null)
            {
                NetworkManagerExtensions.LogError($"Graphical transform cannot be null.");
                return false;
            }
            if (targetTransform == null)
            {
                NetworkManagerExtensions.LogError($"Target transform on {graphicalTransform} cannot be null.");
                return false;
            }
            if (targetTransform == graphicalTransform)
            {
                NetworkManagerExtensions.LogError($"Target transform cannot be the same as graphical transform on {graphicalTransform}.");
                return false;
            }

            return true;
        }
        
        private static void ProcessTransformsOnStart(Transform trackerTransform, Transform targetTransform, Transform graphicalTransform, bool isDetachOnStart)
        {
            if (isDetachOnStart)
            {
                trackerTransform.SetParent(targetTransform);
                        
                TransformProperties gfxWorldProperties = graphicalTransform.GetWorldProperties();
                graphicalTransform.SetParent(null);
                graphicalTransform.SetWorldProperties(gfxWorldProperties);
            }
            else
            {
                Transform trackerParent = graphicalTransform.IsChildOf(targetTransform) ? graphicalTransform.parent : targetTransform;
                trackerTransform.SetParent(trackerParent);
            }

            targetTransform.GetPositionAndRotation(out var pos, out var rot);
            trackerTransform.SetWorldPositionRotationAndScale(pos, rot, graphicalTransform.localScale);
            trackerTransform.gameObject.name = $"{graphicalTransform.name}_Tracker";
        }
        
        private static void ProcessTransformsOnStop(Transform trackerTransform, Transform targetTransform, Transform graphicalTransform, bool isDetachOnStart, bool isAttachOnStop)
        {
            if (trackerTransform == null || targetTransform == null || graphicalTransform == null)
                return;
            if (ApplicationState.IsQuitting())
                return;
            
            trackerTransform.SetParent(null);
            if (isDetachOnStart && isAttachOnStop)
            {
                graphicalTransform.SetParent(targetTransform.parent);
                graphicalTransform.SetLocalProperties(trackerTransform.GetLocalProperties());
            }
        }
        
        /// <summary>
        /// Updates movement settings for a registered smoother.
        /// Both owner and spectator settings are applied atomically.
        /// </summary>
        public void SetSettings(TickSmootherController smoother, in MovementSettings owner, in MovementSettings spectator)
        {
            if (smoother == null) return;
            if (!_lookup.TryGetValue(smoother, out int index)) return;

            _ownerSettings[index] = owner;
            _spectatorSettings[index] = spectator;
        }
        
        /// <summary>
        /// Sets transforms for a registered smoother (target, graphical, tracker).
        /// </summary>
        public void SetTransforms(TickSmootherController smoother, Transform target, Transform graphical)
        {
            if (smoother == null) return;
            if (!_lookup.TryGetValue(smoother, out int index)) return;
            
            bool isDetachOnStart = _detachOnStartMask[index] != 0;
            bool isAttachOnStop = _attachOnStopMask[index] != 0;
            
            Transform tracker = _trackerTaa[index];
            Transform prevTarget = _targetTaa[index];
            Transform prevGraphical = _graphicalTaa[index];
            ProcessTransformsOnStop(tracker, prevTarget, prevGraphical, isDetachOnStart, isAttachOnStop);
            
            _targetTaa[index] = target;
            _graphicalTaa[index] = graphical;
            ProcessTransformsOnStart(tracker, target, graphical, isDetachOnStart);
        }
        
        /// <summary>
        /// Updates the smoothedProperties value.
        /// </summary>
        /// <param name = "smoother">TickSmootherController.</param>
        /// <param name = "value">New value.</param>
        /// <param name = "forOwnerOrOfflineSmoother">True if updating owner smoothing settings, or updating settings on an offline smoother. False to update spectator settings</param>
        public void SetSmoothedProperties(TickSmootherController smoother, TransformPropertiesFlag value, bool forOwnerOrOfflineSmoother)
        {
            if (smoother == null) return;
            if (!_lookup.TryGetValue(smoother, out int index)) return;

            MovementSettings settings = forOwnerOrOfflineSmoother ? _ownerSettings[index] : _spectatorSettings[index];
            settings.SmoothedProperties = value;

            if (forOwnerOrOfflineSmoother)
                _ownerSettings[index] = settings;
            else
                _spectatorSettings[index] = settings;
        }

        /// <summary>
        /// Updates the interpolationValue when not using adaptive interpolation. Calling this method will also disable adaptive interpolation.
        /// </summary>
        /// <param name = "smoother">TickSmootherController.</param>
        /// <param name = "value">New value.</param>
        /// <param name = "forOwnerOrOfflineSmoother">True if updating owner smoothing settings, or updating settings on an offline smoother. False to update spectator settings</param>
        public void SetInterpolationValue(TickSmootherController smoother, byte value, bool forOwnerOrOfflineSmoother) => SetInterpolationValue(smoother, value, forOwnerOrOfflineSmoother, unsetAdaptiveInterpolation: true);

        /// <summary>
        /// Updates the interpolationValue when not using adaptive interpolation. Calling this method will also disable adaptive interpolation.
        /// </summary>
        /// <param name = "smoother">TickSmootherController.</param>
        /// <param name = "value">New value.</param>
        /// <param name = "forOwnerOrOfflineSmoother">True if updating owner smoothing settings, or updating settings on an offline smoother. False to update spectator settings</param>
        /// <param name = "unsetAdaptiveInterpolation"></param>
        private void SetInterpolationValue(TickSmootherController smoother, byte value, bool forOwnerOrOfflineSmoother, bool unsetAdaptiveInterpolation)
        {
            if (smoother == null) return;
            if (!_lookup.TryGetValue(smoother, out int index)) return;
            
            if (value < 1)
                value = 1;

            MovementSettings settings = forOwnerOrOfflineSmoother ? _ownerSettings[index] : _spectatorSettings[index];
            settings.InterpolationValue = value;

            if (forOwnerOrOfflineSmoother)
                _ownerSettings[index] = settings;
            else
                _spectatorSettings[index] = settings;

            if (unsetAdaptiveInterpolation)
                SetAdaptiveInterpolation(smoother, AdaptiveInterpolationType.Off, forOwnerOrOfflineSmoother);
        }

        /// <summary>
        /// Updates the adaptiveInterpolation value.
        /// </summary>
        /// <param name = "smoother">TickSmootherController.</param>
        /// <param name = "value">New value.</param>
        /// <param name = "forOwnerOrOfflineSmoother">True if updating owner smoothing settings, or updating settings on an offline smoother. False to update spectator settings</param>
        public void SetAdaptiveInterpolation(TickSmootherController smoother, AdaptiveInterpolationType value, bool forOwnerOrOfflineSmoother)
        {
            if (smoother == null) return;
            if (!_lookup.TryGetValue(smoother, out int index)) return;
            
            MovementSettings settings = forOwnerOrOfflineSmoother ? _ownerSettings[index] : _spectatorSettings[index];
            settings.AdaptiveInterpolationValue = value;

            if (forOwnerOrOfflineSmoother)
                _ownerSettings[index] = settings;
            else
                _spectatorSettings[index] = settings;

            _updateRealtimeInterpolationPayloads[index] = new UpdateRealtimeInterpolationPayload(1);
        }
        
        /// <summary>
        /// Tries to set local properties for the graphical tracker transform.
        /// </summary>
        /// <param name = "localValues">New values.</param>
        /// <returns>Returns true if the tracker has been setup and values have been applied to teh tracker transform.</returns>
        /// <remarks>When false is returned the values are cached and will be set when tracker is created. A cached value will be used every time the tracker is setup; to disable this behavior call this method with null value.</remarks>
        public bool TrySetGraphicalTrackerLocalProperties(TickSmootherController smoother, TransformProperties? localValues)
        {
            if (smoother == null) return false;
            if (!_lookup.TryGetValue(smoother, out int index)) return false;
            
            if (_trackerTaa[index] == null || localValues == null)
            {
                _queuedTrackerProperties[index] = new NullableTransformProperties(localValues != null, localValues ?? default);
                return false;
            }
            
            _trackerTaa[index].SetLocalProperties(localValues.Value);
            return true;
        }
        
        public bool TryGetGraphicalTrackerLocalProperties(TickSmootherController smoother, out TransformProperties localValues)
        {
            localValues = default;
            if (smoother == null) return false;
            if (!_lookup.TryGetValue(smoother, out int index)) return false;

            Transform trackerTransform = _trackerTaa[index];
            if (trackerTransform != null)
            {
                localValues =  new(trackerTransform.localPosition, trackerTransform.localRotation, trackerTransform.localScale);
                return true;
            }

            NullableTransformProperties queuedTrackerProperties = _queuedTrackerProperties[index];
            if (queuedTrackerProperties.IsExist != 0)
            {
                localValues = queuedTrackerProperties.Properties;
                return true;
            }

            // Fall through.
            return false;
        }
        
        /// <summary>
        /// Marks to teleports the graphical to it's starting position and clears the internal movement queue at the PreTick.
        /// </summary>
        public void Teleport(TickSmootherController smoother)
        {
            if (smoother == null) return;
            if (!_lookup.TryGetValue(smoother, out int index)) return;

            _teleportPayloads[index] = new TeleportPayload(1);
        }
        
        private void ClientManager_OnClientConnectionState(ClientConnectionStateArgs args)
        {
            using (_pm_ClientManager_OnClientConnectionState.Auto())
            {
                while (_trackerTransformsPool.TryPop(out Transform trackerTransform))
                    if (trackerTransform && trackerTransform.gameObject) Destroy(trackerTransform.gameObject);
                
                if (args.ConnectionState == LocalConnectionState.Started)
                    ChangeSubscriptions(true);
                else
                    ChangeSubscriptions(false);
            }
        }

        private void ChangeSubscriptions(bool subscribe)
        {
            if (_timeManager == null)
                return;

            if (_subscribed == subscribe)
                return;

            _subscribed = subscribe;

            if (subscribe)
            {
                _timeManager.OnUpdate += TimeManager_OnUpdate;
                _timeManager.OnPreTick += TimeManager_OnPreTick;
                _timeManager.OnPostTick += TimeManager_OnPostTick;
                _timeManager.OnRoundTripTimeUpdated += TimeManager_OnRoundTripTimeUpdated;

                if (_predictionManager != null)
                    _predictionManager.OnPostReplicateReplay += PredictionManager_OnPostReplicateReplay;
            }
            else
            {
                _timeManager.OnUpdate -= TimeManager_OnUpdate;
                _timeManager.OnPreTick -= TimeManager_OnPreTick;
                _timeManager.OnPostTick -= TimeManager_OnPostTick;
                _timeManager.OnRoundTripTimeUpdated -= TimeManager_OnRoundTripTimeUpdated;

                if (_predictionManager != null)
                    _predictionManager.OnPostReplicateReplay -= PredictionManager_OnPostReplicateReplay;
            }
        }

        /// <summary>
        /// Called every frame.
        /// </summary>
        private void TimeManager_OnUpdate()
        {
            using (_pm_OnUpdate.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return;
                int batchSize = ComputeBatchSize(count);

                var job = new UpdateJob
                {
                    canSmoothMask = _canSmoothMask.AsArray(),
                    deltaTime = Time.deltaTime,
                    
                    moveToTargetPayloads = _moveToTargetPayloads.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(count, batchSize);
                JobHandle moveToTargetHandle = ScheduleMoveToTarget(innerHandle);
                moveToTargetHandle.Complete();
            }
        }

        /// <summary>
        /// Called when the TimeManager invokes OnPreTick.
        /// </summary>
        private void TimeManager_OnPreTick()
        {
            using (_pm_OnPreTick.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return;
                int batchSize = ComputeBatchSize(count);

                for (int i = 0; i < count; i++)
                {
                    Transform graphicalTransform = _graphicalTaa[i];
                    NetworkBehaviour networkBehaviour = _indexToNetworkBehaviour[i];
                    NetworkTransform predictionNetworkTransform = _indexToPredictionNetworkTransform[i];
                    _canSmoothMask[i] = 
                        (byte)(graphicalTransform != null && 
                            (predictionNetworkTransform == null ||
                                !predictionNetworkTransform.DoSettingsAllowSmoothing()) &&
                            _networkManager.IsClientStarted ? 1 : 0);

                    _useOwnerSettingsMask[i] = 
                        (byte)(networkBehaviour == null ||
                            networkBehaviour.IsOwner ||
                            !networkBehaviour.Owner.IsValid ? 1 : 0);
                    
                    _objectReconcilingMask[i] =
                        (byte)(networkBehaviour == null ||
                            networkBehaviour.NetworkObject == null ||
                            networkBehaviour.NetworkObject.IsObjectReconciling ? 1 : 0);
                }
                
                JobHandle preTickMarkHandle = new PreTickMarkJob
                {
                    canSmoothMask = _canSmoothMask.AsArray(),
                    preTickedMask = _preTickedMask.AsArray(),
                    discardExcessivePayloads = _discardExcessivePayloads.AsArray()
                }.Schedule(count, batchSize);
                
                JobHandle discardExcessiveHandle = ScheduleDiscardExcessiveTransformPropertiesQueue(preTickMarkHandle);

                JobHandle teleportHandle = ScheduleTeleport(discardExcessiveHandle);
                
                JobHandle preTickCaptureGraphicalHandle = new PreTickCaptureGraphicalJob
                {
                    canSmoothMask = _canSmoothMask.AsArray(),
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    graphicSnapshot = _preTickGraphicSnapshot.AsArray()
                }.Schedule(_graphicalTaa, teleportHandle);
                
                preTickCaptureGraphicalHandle.Complete();
            }
        }

        /// <summary>
        /// Called when the TimeManager invokes OnPostReplay.
        /// </summary>
        /// <param name = "clientTick">Replay tick for the local client.</param>
        /// <param name = "serverTick"></param>
        /// <remarks>This is dependent on the initializing NetworkBehaviour being set.</remarks>
        private void PredictionManager_OnPostReplicateReplay(uint clientTick, uint serverTick)
        {
            using (_pm_Prediction_OnPostReplicateReplay.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return;
                int batchSize = ComputeBatchSize(count);
                
                var job = new PostReplicateReplayJob
                {
                    clientTick             = clientTick,
                    teleportedTick         = _teleportedTick.AsArray(),
                    objectReconcilingMask  = _objectReconcilingMask.AsArray(),
                    
                    transformProperties = _transformProperties,
                    modifyTransformPropertiesPayloads = _modifyTransformPropertiesPayloads.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(count, batchSize);
                JobHandle modifyTransformPropertiesHandle = ScheduleModifyTransformProperties(innerHandle);
                modifyTransformPropertiesHandle.Complete();
            }
        }

        /// <summary>
        /// Called when TimeManager invokes OnPostTick.
        /// </summary>
        private void TimeManager_OnPostTick()
        {
            using (_pm_OnPostTick.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return;

                JobHandle captureLocalTargetHandle = new CaptureLocalTargetJob
                {
                    canSmoothMask = _canSmoothMask.AsArray(),
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    targetSnapshot = _tempTargetSnapshot.AsArray()
                }.Schedule(_targetTaa);
                
                JobHandle postTickCaptureTrackerHandle = new PostTickCaptureTrackerJob
                {
                    canSmoothMask = _canSmoothMask.AsArray(),
                    detachOnStartMask = _detachOnStartMask.AsArray(),
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    targetSnapshot = _tempTargetSnapshot.AsArray(),
                    trackerSnapshot = _postTickTrackerSnapshot.AsArray()
                }.Schedule(_trackerTaa, captureLocalTargetHandle);
                
                JobHandle postTickHandle = new PostTickJob
                {
                    clientTick              = _timeManager.LocalTick,
                    canSmoothMask           = _canSmoothMask.AsArray(),
                    teleportedTick          = _teleportedTick.AsArray(),
                    preTickedMask           = _preTickedMask.AsArray(),
                    detachOnStartMask       = _detachOnStartMask.AsArray(),
                    postTickTrackerSnapshot = _postTickTrackerSnapshot.AsArray(),
                    preTickGraphicSnapshot  = _preTickGraphicSnapshot.AsArray(),
                    useOwnerSettingsMask    = _useOwnerSettingsMask.AsArray(),
                    ownerSettings           = _ownerSettings.AsArray(),
                    spectatorSettings       = _spectatorSettings.AsArray(),
                    
                    discardExcessivePayloads          = _discardExcessivePayloads.AsArray(),
                    snapNonSmoothedPropertiesPayloads = _snapNonSmoothedPropertiesPayloads.AsArray(),
                    addTransformPropertiesPayloads    =  _addTransformPropertiesPayloads.AsArray()
                }.Schedule(_graphicalTaa, postTickCaptureTrackerHandle);
                
                JobHandle discardExcessiveHandle = ScheduleDiscardExcessiveTransformPropertiesQueue(postTickHandle);
                JobHandle snapNonSmoothedPropertiesHandle = ScheduleSnapNonSmoothedProperties(discardExcessiveHandle);
                JobHandle addTransformPropertiesHandle = ScheduleAddTransformProperties(snapNonSmoothedPropertiesHandle);
                addTransformPropertiesHandle.Complete();
            }
        }

        private void TimeManager_OnRoundTripTimeUpdated(long rttMs)
        {
            using (_pm_TimeManager_OnRoundTripTimeUpdated.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return;
                int batchSize = ComputeBatchSize(count);

                var job = new RoundTripTimeUpdatedJob
                {
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    
                    updateRealtimeInterpolationPayloads = _updateRealtimeInterpolationPayloads.AsArray()
                };

                JobHandle innerHandle = job.Schedule(count, batchSize);
                JobHandle updateRealtimeInterpolationHandle = ScheduleUpdateRealtimeInterpolation(innerHandle);
                updateRealtimeInterpolationHandle.Complete();
            }
        }

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        public JobHandle ScheduleMoveToTarget(in JobHandle outerHandle = default)
        {
            using (_pm_MoveToTarget.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;

                var job = new MoveToTargetJob
                {
                    jobPayloads = _moveToTargetPayloads.AsArray(),
                    
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    
                    realTimeInterpolations = _realTimeInterpolations.AsArray(),
                    moveImmediatelyMask = _moveImmediatelyMask.AsArray(),
                    tickDelta = (float)_timeManager.TickDelta,
                    
                    isMoving = _isMoving.AsArray(),
                    movementMultipliers = _movementMultipliers.AsArray(),
                    
                    transformProperties = _transformProperties,
                    moveRates = _moveRates.AsArray(),
                    
                    setMoveRatesPayloads = _setMoveRatesPayloads.AsArray(),
                    setMovementMultiplierPayloads = _setMovementMultiplierPayloads.AsArray(),
                    clearTransformPropertiesQueuePayloads = _clearTransformPropertiesQueuePayloads.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(_graphicalTaa, outerHandle);
                return innerHandle;
            }
        }
        
        /// <summary>
        /// Updates interpolation based on localClient latency when using adaptive interpolation, or uses set value when adaptive interpolation is off.
        /// </summary>
        public JobHandle ScheduleUpdateRealtimeInterpolation(in JobHandle outerHandle = default)
        {
            using (_pm_ScheduleUpdateRealtimeInterpolation.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                int batchSize = ComputeBatchSize(count);

                var job = new UpdateRealtimeInterpolationJob
                {
                    jobPayloads = _updateRealtimeInterpolationPayloads.AsArray(),
                    
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    
                    tickDelta = (float)_timeManager.TickDelta,
                    tickRate = _timeManager.TickRate,
                    rtt = _timeManager.RoundTripTime,
                    localTick = _timeManager.LocalTick,
                    isServerOnlyStarted = _networkManager.IsServerOnlyStarted,
                    
                    realTimeInterpolations = _realTimeInterpolations.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(count, batchSize, outerHandle);
                return innerHandle;
            }
        }

        /// <summary>
        /// Discards datas over interpolation limit from movement queue.
        /// </summary>
        private JobHandle ScheduleDiscardExcessiveTransformPropertiesQueue(in JobHandle outerHandle = default)
        {
            using (_pm_ScheduleDiscardExcessiveTransformPropertiesQueue.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                int batchSize = ComputeBatchSize(count);
                
                var job = new DiscardExcessiveTransformPropertiesQueueJob
                {
                    jobPayloads = _discardExcessivePayloads.AsArray(),
                    
                    realTimeInterpolations = _realTimeInterpolations.AsArray(),
                    requiredQueuedOverInterpolation = REQUIRED_QUEUED_OVER_INTERPOLATION,
                    
                    transformProperties = _transformProperties,
                    setMoveRatesPayloads = _setMoveRatesPayloads.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(count, batchSize, outerHandle);
                JobHandle setMoveRatesHandle = ScheduleSetMoveRates(innerHandle);
                return setMoveRatesHandle;
            }
        }
        
        /// <summary>
        /// Sets new rates based on next entries in transformProperties queue, against a supplied TransformProperties.
        /// </summary>
        private JobHandle ScheduleSetMoveRates(in JobHandle outerHandle = default)
        {
            using (_pm_ScheduleSetMoveRates.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                int batchSize = ComputeBatchSize(count);
                
                var job = new SetMoveRatesJob
                {
                    jobPayloads = _setMoveRatesPayloads.AsArray(),
                    
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                        
                    transformProperties = _transformProperties,
                    tickDelta = (float)_timeManager.TickDelta,
                    
                    moveRates = _moveRates.AsArray(),
                    setMovementMultiplierPayloads = _setMovementMultiplierPayloads.AsArray(),
                };
                JobHandle innerHandle = job.Schedule(count, batchSize, outerHandle);
                JobHandle setMovementMultiplierHandle = ScheduleSetMovementMultiplier(innerHandle);
                return setMovementMultiplierHandle;
            }
        }
        
        private JobHandle ScheduleSetMovementMultiplier(in JobHandle outerHandle = default)
        {
            using (_pm_ScheduleSetMovementMultiplier.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                int batchSize = ComputeBatchSize(count);
                
                var job = new SetMovementMultiplierJob
                {
                    jobPayloads = _setMovementMultiplierPayloads.AsArray(),
                    
                    transformProperties = _transformProperties,
                    realTimeInterpolations = _realTimeInterpolations.AsArray(),
                    moveImmediatelyMask = _moveImmediatelyMask.AsArray(),
                    
                    movementMultipliers = _movementMultipliers.AsArray()
                };
                JobHandle innerHandle = job.Schedule(count, batchSize, outerHandle);
                return innerHandle;
            }
        }
        
        /// <summary>
        /// Adds a new transform properties and sets move rates if needed.
        /// </summary>
        private JobHandle ScheduleAddTransformProperties(JobHandle outerHandle)
        {
            using (_pm_ScheduleAddTransformProperties.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                
                var job = new AddTransformPropertiesJob
                {
                    jobPayloads = _addTransformPropertiesPayloads.AsArray(),
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    
                    transformProperties = _transformProperties,
                    setMoveRatesPayloads = _setMoveRatesPayloads.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(_graphicalTaa, outerHandle);
                JobHandle setMoveRatesHandle = ScheduleSetMoveRates(innerHandle);
                return setMoveRatesHandle;
            }
        }
        
        /// <summary>
        /// Clears the pending movement queue.
        /// </summary>
        private JobHandle ScheduleClearTransformPropertiesQueue(JobHandle outerHandle)
        {
            using (_pm_ScheduleClearTransformPropertiesQueue.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                int batchSize = ComputeBatchSize(count);
                
                var job = new ClearTransformPropertiesQueueJob
                {
                    jobPayloads = _clearTransformPropertiesQueuePayloads.AsArray(),
                    
                    transformProperties = _transformProperties,
                    moveRates = _moveRates.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(count, batchSize, outerHandle);
                return innerHandle;
            }
        }

        /// <summary>
        /// Modifies a transform property for a tick. This does not error check for empty collections.
        /// firstTick - First tick in the queue. If 0 this will be looked up.
        /// </summary>
        private JobHandle ScheduleModifyTransformProperties(JobHandle outerHandle)
        {
            using (_pm_ScheduleModifyTransformProperties.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                
                JobHandle captureLocalTargetHandle = new CaptureLocalTargetJob
                {
                    canSmoothMask = _canSmoothMask.AsArray(),
                    targetSnapshot = _tempTargetSnapshot.AsArray(),
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray()
                }.Schedule(_targetTaa, outerHandle);

                JobHandle modifyTransformPropertiesHandle = new ModifyTransformPropertiesJob
                {
                    jobPayloads = _modifyTransformPropertiesPayloads.AsArray(),
                    detachOnStartMask = _detachOnStartMask.AsArray(),
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    targetSnapshot = _tempTargetSnapshot.AsArray(),
                    transformProperties = _transformProperties,
                }.Schedule(_trackerTaa, captureLocalTargetHandle);
                    
                return modifyTransformPropertiesHandle;
            }
        }
        
        /// <summary>
        /// Snaps non-smoothed properties to original positoin if setting is enabled.
        /// </summary>
        private JobHandle ScheduleSnapNonSmoothedProperties(JobHandle outerHandle)
        {
            using (_pm_ScheduleSnapNonSmoothedProperties.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                
                var job = new SnapNonSmoothedPropertiesJob
                {
                    jobPayloads = _snapNonSmoothedPropertiesPayloads.AsArray(),
                    
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                };
                
                JobHandle innerHandle = job.Schedule(_graphicalTaa, outerHandle);
                return innerHandle;
            }
        }
        
        /// <summary>
        /// Teleports the graphical to it's starting position and clears the internal movement queue.
        /// </summary>
        private JobHandle ScheduleTeleport(JobHandle outerHandle)
        {
            using (_pm_ScheduleTeleport.Auto())
            {
                int count = _indexToSmoother.Count;
                if (count == 0) return outerHandle;
                
                var job = new TeleportJob
                {
                    jobPayloads = _teleportPayloads.AsArray(),
                    
                    useOwnerSettingsMask = _useOwnerSettingsMask.AsArray(),
                    ownerSettings = _ownerSettings.AsArray(),
                    spectatorSettings = _spectatorSettings.AsArray(),
                    preTickTrackerSnapshot = _postTickTrackerSnapshot.AsArray(),
                    localTick = _timeManager.LocalTick,
                    
                    transformProperties = _transformProperties,
                    clearTransformPropertiesQueuePayloads = _clearTransformPropertiesQueuePayloads.AsArray(),
                    moveRates = _moveRates.AsArray(),
                    teleportedTick = _teleportedTick.AsArray()
                };
                
                JobHandle innerHandle = job.Schedule(_graphicalTaa, outerHandle);
                JobHandle clearTransformPropertiesQueueHandle = ScheduleClearTransformPropertiesQueue(innerHandle);
                return clearTransformPropertiesQueueHandle;
            }
        }
        
        private static int ComputeBatchSize(int length, int minBatch = 1, int maxBatch = 128)
        {
            if (length <= 0) return 1;

            // +1: main thread + worker threads
            int workers = JobsUtility.JobWorkerCount + 1;

            // Aim for ~4 waves of batches across all workers.
            int targetBatches = Mathf.Max(1, workers * 4);

            // CeilDiv to get iterations per batch
            int batch = (length + targetBatches - 1) / targetBatches;

            return Mathf.Clamp(batch, minBatch, maxBatch);
        }
    }
}
#endif