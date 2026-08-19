#if !FISHNET_THREADED_TICKSMOOTHERS
using System;
using FishNet.Managing;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Object.Prediction;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using Unity.Profiling;
using UnityEngine;
using UnityEngine.Scripting;

namespace FishNet.Component.Transforming.Beta
{
    /// <summary>
    /// This class is under regular development and it's API may change at any time.
    /// </summary>
    public sealed class UniversalTickSmoother : IResettable
    {
        #region Types.
        [Preserve]
        private struct TickTransformProperties
        {
            public readonly uint Tick;
            public readonly TransformProperties Properties;

            public TickTransformProperties(uint tick, Transform t)
            {
                Tick = tick;
                Properties = new(t.localPosition, t.localRotation, t.localScale);
            }

            public TickTransformProperties(uint tick, Transform t, Vector3 localScale)
            {
                Tick = tick;
                Properties = new(t.localPosition, t.localRotation, localScale);
            }

            public TickTransformProperties(uint tick, TransformProperties tp)
            {
                Tick = tick;
                Properties = tp;
            }

            public TickTransformProperties(uint tick, TransformProperties tp, Vector3 localScale)
            {
                Tick = tick;
                tp.Scale = localScale;
                Properties = tp;
            }
        }
        #endregion

        #region public.
        /// <summary>
        /// True if currently initialized.
        /// </summary>
        public bool IsInitialized { get; private set; }
        #endregion

        #region Private.
        /// <summary>
        /// How quickly to move towards goal values.
        /// </summary>
        private MoveRates _moveRates = new();
        /// <summary>
        /// True if a pretick occurred since last postTick.
        /// </summary>
        private bool _preTicked;
        /// <summary>
        /// World values of the graphical after it's been aligned to initialized values in PreTick.
        /// </summary>
        private TransformProperties _trackerPreTickWorldValues;
        /// <summary>
        /// World values of the graphical after it's been aligned to initialized values in PreTick.
        /// </summary>
        private TransformProperties _graphicsPreTickWorldValues;
        /// <summary>
        /// Cached value of adaptive interpolation value.
        /// </summary>
        private AdaptiveInterpolationType _cachedAdaptiveInterpolationValue;
        /// <summary>
        /// Cached value of flat interpolation value.
        /// </summary>
        private byte _cachedInterpolationValue;
        /// <summary>
        /// Cached properties to smooth of the graphical.
        /// </summary>
        private TransformPropertiesFlag _cachedSmoothedProperties;
        /// <summary>
        /// Cached value of snapping non-smoothed properties.
        /// </summary>
        private bool _cachedSnapNonSmoothedProperties;
        /// <summary>
        /// Squared distance target must travel to cause a teleport.
        /// </summary>
        private float _cachedTeleportThreshold;
        /// <summary>
        /// True if to detach on network start.
        /// </summary>
        private bool _detachOnStart;
        /// <summary>
        /// True to re-attach on network stop.
        /// </summary>
        private bool _attachOnStop;
        /// <summary>
        /// True to begin moving soon as movement data becomes available. Movement will ease in until at interpolation value. False to prevent movement until movement data count meet interpolation.
        /// </summary>
        private bool _moveImmediately;
        /// <summary>
        /// Transform the graphics should follow.
        /// </summary>
        private Transform _targetTransform;
        /// <summary>
        /// Cached value of the object to smooth.
        /// </summary>
        private Transform _graphicalTransform;
        /// <summary>
        /// Empty gameObject containing a transform which has properties checked after each simulation.
        /// If the graphical starts off as nested of targetTransform then this object is created where the graphical object is.
        /// Otherwise, this object is placed directly beneath targetTransform.
        /// </summary>
        private Transform _trackerTransform;
        /// <summary>
        /// TimeManager tickDelta.
        /// </summary>
        private float _tickDelta;
        /// <summary>
        /// NetworkBehaviour this is initialized for. Value may be null.
        /// </summary>
        private NetworkBehaviour _initializingNetworkBehaviour;
        /// <summary>
        /// TimeManager this is initialized for.
        /// </summary>
        private TimeManager _initializingTimeManager;
        /// <summary>
        /// Value to multiply movement by. This is used to reduce or increase the rate the movement buffer is consumed.
        /// </summary>
        private float _movementMultiplier = 1f;
        /// <summary>
        /// TransformProperties to move towards.
        /// </summary>
        private BasicQueue<TickTransformProperties> _transformProperties;
        /// <summary>
        /// Last tick this was teleported on.
        /// </summary>
        private uint _teleportedTick = TimeManager.UNSET_TICK;
        /// <summary>
        /// Current interpolation value, be it a flat value or adaptive.
        /// </summary>
        private byte _realtimeInterpolation;
        /// <summary>
        /// Settings to use for owners.
        /// </summary>
        private MovementSettings _controllerMovementSettings;
        /// <summary>
        /// Settings to use for spectators.
        /// </summary>
        private MovementSettings _spectatorMovementSettings;
        /// <summary>
        /// True if moving has started and has not been stopped.
        /// </summary>
        private bool _isMoving;
        /// <summary>
        /// NetworkTransform used when prediction type is set to other.
        /// </summary>
        private NetworkTransform _predictionNetworkTransform;
        #endregion

        #region Private Profiler Markers
        // private static readonly ProfilerMarker _pm_ConsumeFixedOffset = new("UniversalTickSmoother.ConsumeFixedOffset(uint)");
        // private static readonly ProfilerMarker _pm_AxiswiseClamp = new("UniversalTickSmoother.AxiswiseClamp(TransformProperties, TransformProperties)");
        private static readonly ProfilerMarker _pm_UpdateRealtimeInterpolation = new("UniversalTickSmoother.UpdateRealtimeInterpolation()");
        private static readonly ProfilerMarker _pm_OnUpdate = new("UniversalTickSmoother.OnUpdate(float)");
        private static readonly ProfilerMarker _pm_OnPreTick = new("UniversalTickSmoother.OnPreTick()");
        private static readonly ProfilerMarker _pm_OnPostReplicateReplay = new("UniversalTickSmoother.OnPostReplicateReplay(uint)");
        private static readonly ProfilerMarker _pm_OnPostTick = new("UniversalTickSmoother.OnPostTick(uint)");
        private static readonly ProfilerMarker _pm_ClearTPQ = new("UniversalTickSmoother.ClearTransformPropertiesQueue()");
        private static readonly ProfilerMarker _pm_DiscardTPQ = new("UniversalTickSmoother.DiscardExcessiveTransformPropertiesQueue()");
        private static readonly ProfilerMarker _pm_AddTP = new("UniversalTickSmoother.AddTransformProperties(uint, TransformProperties, TransformProperties)");
        private static readonly ProfilerMarker _pm_ModifyTP = new("UniversalTickSmoother.ModifyTransformProperties(uint, uint)");
        private static readonly ProfilerMarker _pm_SetMoveRates = new("UniversalTickSmoother.SetMoveRates(in TransformProperties)");
        private static readonly ProfilerMarker _pm_MoveToTarget = new("UniversalTickSmoother.MoveToTarget(float)");
        #endregion

        #region Const.
        /// <summary>
        /// Maximum allowed entries to be queued over the interpolation amount.
        /// </summary>
        private const int MAXIMUM_QUEUED_OVER_INTERPOLATION = 3;
        #endregion

        [Preserve]
        public UniversalTickSmoother() { }

        ~UniversalTickSmoother()
        {
            // This is a last resort for if something didnt deinitialize right.
            ResetState();
        }

        [Obsolete("This method is no longer used. Use TrySetGraphicalTrackerLocalProperties(TransformProperties).")] // Remove V5
        public void SetGraphicalInitializedOffsetValues(TransformProperties value) { }

        [Obsolete("This method is no longer used. Use GetGraphicalTrackerLocalProperties.")] // Remove V5
        public TransformProperties GetGraphicalInitializedOffsetValues() => default;

        /// <summary>
        /// Tries to set local properties for the graphical tracker transform.
        /// </summary>
        /// <param name = "localValues">New values.</param>
        /// <returns>Returns true if the tracker has been setup and values have been applied to teh tracker transform.</returns>
        /// <remarks>When false is returned the values are cached and will be set when tracker is created. A cached value will be used every time the tracker is setup; to disable this behavior call this method with null value.</remarks>
        public bool TrySetGraphicalTrackerLocalProperties(TransformProperties? localValues)
        {
            if (_trackerTransform == null || localValues == null)
            {
                _queuedTrackerProperties = localValues;
                return false;
            }


            _trackerTransform.SetLocalProperties(localValues.Value);
            return true;
        }

        [Obsolete("This method is no longer used. Use TrySetGraphicalTrackerLocalProperties(TransformProperties).")] // Remove V5
        public void SetAdditionalGraphicalOffsetValues(TransformProperties localValues) { }

        [Obsolete("This method is no longer used. Use GetGraphicalTrackerLocalProperties.")] // Remove V5
        public TransformProperties GetAdditionalGraphicalOffsetValues() => default;

        public TransformProperties GetGraphicalTrackerLocalProperties()
        {
            if (_trackerTransform != null)
                return new(_trackerTransform.localPosition, _trackerTransform.localRotation, _trackerTransform.localScale);
            if (_queuedTrackerProperties != null)
                return _queuedTrackerProperties.Value;

            // Fall through.
            NetworkManager manager = _initializingNetworkBehaviour == null ? null : _initializingNetworkBehaviour.NetworkManager;
            manager.LogWarning($"Graphical tracker properties cannot be returned because tracker is not setup yet, and no setup properties have been specified. Use TrySetGraphicalTrackerProperties to set setup properties or call this method after IsInitialized is true.");
            return default;
        }

        /// <summary>
        /// Properties for the tracker which are queued to be set when the tracker is setup.
        /// </summary>
        private TransformProperties? _queuedTrackerProperties;

        /// <summary>
        /// Updates the smoothedProperties value.
        /// </summary>
        /// <param name = "value">New value.</param>
        /// <param name = "forController">True if updating owner smoothing settings, or updating settings on an offline smoother. False to update spectator settings</param>
        public void SetSmoothedProperties(TransformPropertiesFlag value, bool forController)
        {
            _controllerMovementSettings.SmoothedProperties = value;
            SetCaches(forController);
        }

        /// <summary>
        /// Updates the interpolationValue when not using adaptive interpolation. Calling this method will also disable adaptive interpolation.
        /// </summary>
        /// <param name = "value"></param>
        public void SetInterpolationValue(byte value, bool forOwnerOrOfflineSmoother) => SetInterpolationValue(value, forOwnerOrOfflineSmoother, unsetAdaptiveInterpolation: true);

        /// <summary>
        /// Updates the interpolationValue when not using adaptive interpolation. Calling this method will also disable adaptive interpolation.
        /// </summary>
        private void SetInterpolationValue(byte value, bool forOwnerOrOfflineSmoother, bool unsetAdaptiveInterpolation)
        {
            if (value < 1)
                value = 1;

            if (forOwnerOrOfflineSmoother)
                _controllerMovementSettings.InterpolationValue = value;
            else
                _spectatorMovementSettings.InterpolationValue = value;

            if (unsetAdaptiveInterpolation)
                SetAdaptiveInterpolation(AdaptiveInterpolationType.Off, forOwnerOrOfflineSmoother);
        }

        /// <summary>
        /// Updates the adaptiveInterpolation value.
        /// </summary>
        /// <param name = "adaptiveInterpolation">New value.</param>
        public void SetAdaptiveInterpolation(AdaptiveInterpolationType value, bool forOwnerOrOfflineSmoother)
        {
            if (forOwnerOrOfflineSmoother)
                _controllerMovementSettings.AdaptiveInterpolationValue = value;
            else
                _spectatorMovementSettings.AdaptiveInterpolationValue = value;

            UpdateRealtimeInterpolation();
        }

        public void Initialize(InitializationSettings initializationSettings, MovementSettings controllerSettings, MovementSettings spectatorSettings)
        {
            ResetState();

            Transform graphicalTransform = initializationSettings.GraphicalTransform;
            Transform targetTransform = initializationSettings.TargetTransform;

            if (!TransformsAreValid(graphicalTransform, targetTransform))
                return;

            _transformProperties = CollectionCaches<TickTransformProperties>.RetrieveBasicQueue();
            _controllerMovementSettings = controllerSettings;
            _spectatorMovementSettings = spectatorSettings;

            /* Unset scale smoothing if not detaching. This is to prevent
             * the scale from changing with the parent if nested, as that
             * would result in the scale being modified twice, once on the parent
             * and once on the graphical. Thanks deo_wh for find! */
            if (!initializationSettings.DetachOnStart)
            {
                _controllerMovementSettings.SmoothedProperties &= ~TransformPropertiesFlag.Scale;
                _spectatorMovementSettings.SmoothedProperties &= ~TransformPropertiesFlag.Scale;
            }

            _initializingNetworkBehaviour = initializationSettings.InitializingNetworkBehaviour;
            _initializingTimeManager = initializationSettings.InitializingTimeManager;
            _targetTransform = targetTransform;
            _graphicalTransform = graphicalTransform;
            _tickDelta = (float)initializationSettings.InitializingTimeManager.TickDelta;
            _detachOnStart = initializationSettings.DetachOnStart;
            _attachOnStop = initializationSettings.AttachOnStop;
            _moveImmediately = initializationSettings.MoveImmediately;
 
            if (initializationSettings.FavorPredictionNetworkTransform && _initializingNetworkBehaviour != null)
            {
                NetworkObject networkObject = _initializingNetworkBehaviour.NetworkObject;
                if (!networkObject.IsRigidbodyPredictionType)
                    _predictionNetworkTransform = networkObject.PredictionNetworkTransform;
                else
                    _predictionNetworkTransform = null;
            }
            else
            {
                _predictionNetworkTransform = null;
            }
            
            SetCaches(GetUseOwnerSettings());

            //Use set method as it has sanity checks.
            SetInterpolationValue(_controllerMovementSettings.InterpolationValue, forOwnerOrOfflineSmoother: true, unsetAdaptiveInterpolation: false);
            SetInterpolationValue(_spectatorMovementSettings.InterpolationValue, forOwnerOrOfflineSmoother: false, unsetAdaptiveInterpolation: false);

            SetAdaptiveInterpolation(_controllerMovementSettings.AdaptiveInterpolationValue, forOwnerOrOfflineSmoother: true);
            SetAdaptiveInterpolation(_spectatorMovementSettings.AdaptiveInterpolationValue, forOwnerOrOfflineSmoother: false);

            SetupTrackerTransform();
            /* This is called after setting up the tracker transform in the scenario
             * the user set additional offsets before this was initialized. */
            if (_queuedTrackerProperties != null)
                TrySetGraphicalTrackerLocalProperties(_queuedTrackerProperties.Value);

            void SetupTrackerTransform()
            {
                _trackerTransform = new GameObject($"{_graphicalTransform.name}_Tracker").transform;

                if (_detachOnStart)
                {
                    _trackerTransform.SetParent(_targetTransform);
                }
                else
                {
                    Transform trackerParent = _graphicalTransform.IsChildOf(targetTransform) ? _graphicalTransform.parent : targetTransform;
                    _trackerTransform.SetParent(trackerParent);
                }

                _trackerTransform.SetLocalPositionRotationAndScale(_graphicalTransform.localPosition, graphicalTransform.localRotation, graphicalTransform.localScale);
            }

            IsInitialized = true;
        }

        /// <summary>
        /// Returns if configured transforms are valid.
        /// </summary>
        /// <returns></returns>
        private bool TransformsAreValid(Transform graphicalTransform, Transform targetTransform)
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

        /// <summary>
        /// Returns true if to use adaptive interpolation.
        /// </summary>
        /// <returns></returns>
        private bool GetUseAdaptiveInterpolation()
        {
            if (_cachedAdaptiveInterpolationValue == AdaptiveInterpolationType.Off || _initializingTimeManager.NetworkManager.IsServerOnlyStarted)
                return false;

            return true;
        }

        /// <summary>
        /// Gets if to use owner values.
        /// </summary>
        /// <remarks>OwnerSettings can be used to read determine this as both owner and spectator settings will have the name InitializingNetworkBehaviour.</remarks>
        /// <returns></returns>
        private bool GetUseOwnerSettings()
        {
            /* No networkBehaviour indicates an offline smoother.
             * The offline smoothers use owner settings. */
            if (_initializingNetworkBehaviour == null)
                return true;

            if (_initializingNetworkBehaviour.IsController)
                return true;

            return false;
            //            return _initializingNetworkBehaviour.IsOwner || !_initializingNetworkBehaviour.Owner.IsValid;
        }

        /// <summary>
        /// Updates OwnerDuringPreTick value and caches if needed.
        /// </summary>
        private void SetCaches(bool useOwnerSettings)
        {
            MovementSettings movementSettings = useOwnerSettings ? _controllerMovementSettings : _spectatorMovementSettings;

            _cachedSmoothedProperties = movementSettings.SmoothedProperties;
            _cachedSnapNonSmoothedProperties = movementSettings.SnapNonSmoothedProperties;
            _cachedAdaptiveInterpolationValue = movementSettings.AdaptiveInterpolationValue;
            _cachedInterpolationValue = movementSettings.InterpolationValue;

            _cachedTeleportThreshold = movementSettings.EnableTeleport ? movementSettings.TeleportThreshold * movementSettings.TeleportThreshold : MoveRates.UNSET_VALUE;
        }

        /// <summary>
        /// Deinitializes this smoother resetting values.
        /// </summary>
        public void Deinitialize()
        {
            ResetState();
            IsInitialized = false;
        }

        /// <summary>
        /// Updates interpolation based on localClient latency when using adaptive interpolation, or uses set value when adaptive interpolation is off.
        /// </summary>
        public void UpdateRealtimeInterpolation()
        {
            using (_pm_UpdateRealtimeInterpolation.Auto())
            {
                /*  If not networked, server is started, or if not
                 * using adaptive interpolation then use
                 * flat interpolation.*/
                if (!GetUseAdaptiveInterpolation())
                {
                    _realtimeInterpolation = _cachedInterpolationValue;
                    return;
                }

                /* If here then adaptive interpolation is being calculated. */

                TimeManager tm = _initializingTimeManager;

                //Calculate roughly what client state tick would be.
                uint localTick = tm.LocalTick;
                //This should never be the case; this is a precautionary against underflow.
                if (localTick == TimeManager.UNSET_TICK)
                    return;

                //Ensure at least 1 tick.
                long rttTime = tm.RoundTripTime;
                uint rttTicks = tm.TimeToTicks(rttTime) + 1;

                uint clientStateTick = localTick - rttTicks;
                float interpolation = localTick - clientStateTick;

                //Minimum interpolation is that of adaptive interpolation level.
                interpolation += (byte)_cachedAdaptiveInterpolationValue;

                //Ensure interpolation is not more than a second.
                if (interpolation > tm.TickRate)
                    interpolation = tm.TickRate;
                else if (interpolation > byte.MaxValue)
                    interpolation = byte.MaxValue;

                /* Only update realtime interpolation if it changed more than 1
                 * tick. This is to prevent excessive changing of interpolation value, which
                 * could result in noticeable speed ups/slow downs given movement multiplier
                 * may change when buffer is too full or short. */
                if (_realtimeInterpolation == 0 || Math.Abs(_realtimeInterpolation - interpolation) > 1)
                    _realtimeInterpolation = (byte)Math.Ceiling(interpolation);
            }
        }

        /// <summary>
        /// This should be called when OnStartClient is invoked on the initializing NetworkBehaviour.
        /// </summary>
        /// <remarks>This does not need to be called if there is no initializing NetworkBehaviour.</remarks>
        public void StartSmoother()
        {
            DetachOnStart();
        }

        /// <summary>
        /// This should be called when OnStopClient is invoked on the initializing NetworkBehaviour.
        /// </summary>
        /// <remarks>This does not need to be called if there is no initializing NetworkBehaviour.</remarks>
        internal void StopSmoother()
        {
            AttachOnStop();
        }

        /// <summary>
        /// Called every frame.
        /// </summary>
        public void OnUpdate(float delta)
        {
            using (_pm_OnUpdate.Auto())
            {
                if (!CanSmooth())
                    return;

                MoveToTarget(delta);
            }
        }

        /// <summary>
        /// Called when the TimeManager invokes OnPreTick.
        /// </summary>
        public void OnPreTick()
        {
            using (_pm_OnPreTick.Auto())
            {
                if (!CanSmooth())
                    return;

                SetCaches(GetUseOwnerSettings());

                _preTicked = true;
                DiscardExcessiveTransformPropertiesQueue();
                _graphicsPreTickWorldValues = _graphicalTransform.GetWorldProperties();
                _trackerPreTickWorldValues = GetTrackerWorldProperties();
            }
        }

        /// <summary>
        /// Called when the TimeManager invokes OnPostReplay.
        /// </summary>
        /// <param name = "clientTick">Replay tick for the local client.</param>
        /// <remarks>This is dependent on the initializing NetworkBehaviour being set.</remarks>
        public void OnPostReplicateReplay(uint clientTick)
        {
            using (_pm_OnPostReplicateReplay.Auto())
            {
                if (!NetworkObjectIsReconciling())
                    return;

                if (_transformProperties.Count == 0)
                    return;
                if (clientTick <= _teleportedTick)
                    return;
                uint firstTick = _transformProperties.Peek().Tick;
                //Already in motion to first entry, or first entry passed tick.
                if (clientTick <= firstTick)
                    return;

                ModifyTransformProperties(clientTick, firstTick);
            }
        }

        /// <summary>
        /// Called when TimeManager invokes OnPostTick.
        /// </summary>
        /// <param name = "clientTick">Local tick of the client.</param>
        public void OnPostTick(uint clientTick)
        {
            using (_pm_OnPostTick.Auto())
            {
                if (!CanSmooth())
                    return;
                if (clientTick <= _teleportedTick)
                    return;

                //If preticked then previous transform values are known.
                if (_preTicked)
                {
                    DiscardExcessiveTransformPropertiesQueue();

                    //Only needs to be put to pretick position if not detached.
                    if (!_detachOnStart)
                        _graphicalTransform.SetWorldProperties(_graphicsPreTickWorldValues);

                    //SnapNonSmoothedProperties();
                    AddTransformProperties(clientTick);
                }
                //If did not pretick then the only thing we can do is snap to instantiated values.
                else
                {
                    //Only set to position if not to detach.
                    if (!_detachOnStart)
                        _graphicalTransform.SetWorldProperties(GetTrackerWorldProperties());
                }
            }
        }

        /// <summary>
        /// Snaps non-smoothed properties to original positoin if setting is enabled.
        /// </summary>
        private void SnapNonSmoothedProperties()
        {
            //Feature is not enabled.
            if (!_cachedSnapNonSmoothedProperties)
                return;

            TransformPropertiesFlag smoothedProperties = _cachedSmoothedProperties;

            //Everything is smoothed.
            if (smoothedProperties == TransformPropertiesFlag.Everything)
                return;

            TransformProperties goalValeus = GetTrackerWorldProperties();

            if (!smoothedProperties.FastContains(TransformPropertiesFlag.Position))
                _graphicalTransform.position = goalValeus.Position;
            if (!smoothedProperties.FastContains(TransformPropertiesFlag.Rotation))
                _graphicalTransform.rotation = goalValeus.Rotation;
            if (!smoothedProperties.FastContains(TransformPropertiesFlag.Scale))
                _graphicalTransform.localScale = goalValeus.Scale;
        }

        /// <summary>
        /// Returns if the initialized NetworkBehaviour's NetworkObject is reconcilling.
        /// </summary>
        private bool NetworkObjectIsReconciling() => _initializingNetworkBehaviour == null || _initializingNetworkBehaviour.NetworkObject.IsObjectReconciling;

        /// <summary>
        /// Teleports the graphical to it's starting position and clears the internal movement queue.
        /// </summary>
        public void Teleport()
        {
            if (_initializingTimeManager == null)
                return;

            //If using adaptive interpolation then set the tick which was teleported.
            if (_controllerMovementSettings.AdaptiveInterpolationValue != AdaptiveInterpolationType.Off)
            {
                TimeManager tm = _initializingTimeManager == null ? InstanceFinder.TimeManager : _initializingTimeManager;
                if (tm != null)
                    _teleportedTick = tm.LocalTick;
            }

            ClearTransformPropertiesQueue();

            _graphicalTransform.SetWorldProperties(_trackerTransform.GetWorldProperties());
        }

        /// <summary>
        /// Clears the pending movement queue.
        /// </summary>
        private void ClearTransformPropertiesQueue()
        {
            using (_pm_ClearTPQ.Auto())
            {
                _transformProperties.Clear();
                //Also unset move rates since there is no more queue.
                _moveRates = new(MoveRates.UNSET_VALUE);
            }
        }

        /// <summary>
        /// Discards datas over interpolation limit from movement queue.
        /// </summary>
        private void DiscardExcessiveTransformPropertiesQueue()
        {
            using (_pm_DiscardTPQ.Auto())
            {
                int propertiesCount = _transformProperties.Count;
                int dequeueCount = propertiesCount - (_realtimeInterpolation + MAXIMUM_QUEUED_OVER_INTERPOLATION);

                //If there are entries to dequeue.
                if (dequeueCount > 0)
                {
                    TickTransformProperties tpp = default;
                    for (int i = 0; i < dequeueCount; i++)
                        tpp = _transformProperties.Dequeue();

                    SetMoveRates(tpp.Properties);
                }
            }
        }

        /// <summary>
        /// Adds a new transform properties and sets move rates if needed.
        /// </summary>
        private void AddTransformProperties(uint tick)
        {
            using (_pm_AddTP.Auto())
            {
                TickTransformProperties tpp = new(tick, GetTrackerWorldProperties());
                _transformProperties.Enqueue(tpp);

                //If first entry then set move rates.
                if (_transformProperties.Count == 1)
                {
                    TransformProperties gfxWorldProperties = _graphicalTransform.GetWorldProperties();
                    SetMoveRates(gfxWorldProperties);
                }
            }
        }

        /// <summary>
        /// Modifies a transform property for a tick. This does not error check for empty collections.
        /// </summary>
        /// <param name = "firstTick">First tick in the queue. If 0 this will be looked up.</param>
        private void ModifyTransformProperties(uint clientTick, uint firstTick)
        {
            using (_pm_ModifyTP.Auto())
            {
                int queueCount = _transformProperties.Count;
                uint tick = clientTick;
                /*Ticks will always be added incremental by 1 so it's safe to jump ahead the difference
                 * of tick and firstTick. */
                int index = (int)(tick - firstTick);
                //Replace with new data.
                if (index < queueCount)
                {
                    if (tick != _transformProperties[index].Tick)
                    {
                        //Should not be possible.
                    }
                    else
                    {
                        TransformProperties newProperties = GetTrackerWorldProperties();
                        /* Adjust transformProperties to ease into any corrections.
                         * The corrected value is used the more the index is to the end
                         * of the queue. */
                        /* We want to be fully eased in by the last entry of the queue. */

                        int lastPossibleIndex = queueCount - 1;
                        int adjustedQueueCount = lastPossibleIndex - 1;
                        if (adjustedQueueCount < 1)
                            adjustedQueueCount = 1;
                        float easePercent = (float)index / adjustedQueueCount;

                        //If easing.
                        if (easePercent < 1f)
                        {
                            if (easePercent < 1f)
                                easePercent = (float)Math.Pow(easePercent, adjustedQueueCount - index);

                            TransformProperties oldProperties = _transformProperties[index].Properties;
                            newProperties.Position = Vector3.Lerp(oldProperties.Position, newProperties.Position, easePercent);
                            newProperties.Rotation = Quaternion.Lerp(oldProperties.Rotation, newProperties.Rotation, easePercent);
                            newProperties.Scale = Vector3.Lerp(oldProperties.Scale, newProperties.Scale, easePercent);
                        }

                        _transformProperties[index] = new(tick, newProperties);
                    }
                }
                else
                {
                    //This should never happen.
                }
            }
        }

        /// <summary>
        /// Gets properties of the tracker.
        /// </summary>
        private TransformProperties GetTrackerWorldProperties()
        {
            /* Return lossyScale if graphical is not attached. Otherwise,
             * graphical should retain the tracker localScale so it changes
             * with root. */

            Vector3 scale = _detachOnStart ? _trackerTransform.lossyScale : _trackerTransform.localScale;
            return new(_trackerTransform.position, _trackerTransform.rotation, scale);
        }

        /// <summary>
        /// Returns if prediction can be used on this rigidbody.
        /// </summary>
        /// <returns></returns>
        private bool CanSmooth()
        {
            //No graphical object is set.
            if (_graphicalTransform == null)
                return false;

            /* When this is the case the prediction networkTransform exist and is
             * configured in a way to smooth the object, therefor this component should not be smoothing. */
            if (_predictionNetworkTransform != null && _predictionNetworkTransform.DoSettingsAllowSmoothing())
                return false;

            return _initializingTimeManager.NetworkManager.IsClientStarted;
        }

        /// <summary>
        /// Sets new rates based on next entries in transformProperties queue, against a supplied TransformProperties.
        /// </summary>
        private void SetMoveRates(in TransformProperties prevValues)
        {
            using (_pm_SetMoveRates.Auto())
            {
                if (_transformProperties.Count == 0)
                {
                    _moveRates = new(MoveRates.UNSET_VALUE);
                    return;
                }

                TransformProperties nextValues = _transformProperties.Peek().Properties;

                float duration = _tickDelta;

                _moveRates = MoveRates.GetMoveRates(prevValues, nextValues, duration, _cachedTeleportThreshold);
                _moveRates.TimeRemaining = duration;

                SetMovementMultiplier();
            }
        }

        private void SetMovementMultiplier()
        {
            if (_moveImmediately)
            {
                float percent = Mathf.InverseLerp(0, _realtimeInterpolation, _transformProperties.Count);
                _movementMultiplier = percent;

                _movementMultiplier = Mathf.Clamp(_movementMultiplier, 0.5f, 1.05f);
            }
            //For the time being, not moving immediately uses these multiplier calculations.
            else
            {
                /* If there's more in queue than interpolation then begin to move faster based on overage.
                 * Move 5% faster for every overage. */
                int overInterpolation = _transformProperties.Count - _realtimeInterpolation;
                //If needs to be adjusted.
                if (overInterpolation != 0)
                {
                    _movementMultiplier += 0.015f * overInterpolation;
                }
                //If does not need to be adjusted.
                else
                {
                    //If interpolation is 1 then slow down just barely to accomodate for frame delta variance.
                    if (_realtimeInterpolation == 1)
                        _movementMultiplier = 1f;
                }

                _movementMultiplier = Mathf.Clamp(_movementMultiplier, 0.95f, 1.05f);
            }
        }

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        private void MoveToTarget(float delta)
        {
            using (_pm_MoveToTarget.Auto())
            {
                int tpCount = _transformProperties.Count;

                //No data.
                if (tpCount == 0)
                    return;

                if (_moveImmediately)
                {
                    _isMoving = true;
                }
                else
                {
                    //Enough in buffer to move.
                    if (tpCount >= _realtimeInterpolation)
                    {
                        _isMoving = true;
                    }
                    else if (!_isMoving)
                    {
                        return;
                    }
                    /* If buffer is considerably under goal then halt
                     * movement. This will allow the buffer to grow. */
                    else if (tpCount - _realtimeInterpolation < -4)
                    {
                        _isMoving = false;
                        return;
                    }
                }

                TickTransformProperties ttp = _transformProperties.Peek();

                TransformPropertiesFlag smoothedProperties = _cachedSmoothedProperties;

                _moveRates.Move(_graphicalTransform, ttp.Properties, smoothedProperties, delta * _movementMultiplier, useWorldSpace: true);

                float tRemaining = _moveRates.TimeRemaining;
                //if TimeLeft is <= 0f then transform is at goal. Grab a new goal if possible.
                if (tRemaining <= 0f)
                {
                    //Dequeue current entry and if there's another call a move on it.
                    _transformProperties.Dequeue();

                    //If there are entries left then setup for the next.
                    if (_transformProperties.Count > 0)
                    {
                        SetMoveRates(ttp.Properties);
                        //If delta is negative then call move again with abs.
                        if (tRemaining < 0f)
                            MoveToTarget(Mathf.Abs(tRemaining));
                    }
                    //No remaining, set to snap.
                    else
                    {
                        ClearTransformPropertiesQueue();
                    }
                }
            }
        }

        private void DetachOnStart()
        {
            if (!_detachOnStart)
                return;

            TransformProperties gfxWorldProperties = _graphicalTransform.GetWorldProperties();
            _graphicalTransform.SetParent(null);
            _graphicalTransform.SetWorldProperties(gfxWorldProperties);
        }

        /// <summary>
        /// Attaches to Target transform if possible.
        /// </summary>
        private void AttachOnStop()
        {
            //Never detached.
            if (!_detachOnStart)
                return;
            //Graphical is null, nothing can be moved.
            if (_graphicalTransform == null)
                return;
            if (ApplicationState.IsQuitting())
                return;
            
            /* If not to re-attach or if there's no target to reference
             * then the graphical must be destroyed. */
            bool destroy = !_attachOnStop || _targetTransform == null;
            //If not to re-attach then destroy graphical if needed.
            if (destroy)
            {
                UnityEngine.Object.Destroy(_graphicalTransform.gameObject);
                return;
            }

            /* This can occasionally cause an error:
             * Cannot set the parent of the GameObject 'XYZ' while its new parent 'ABC' is being destroyed
             *
             * There is nothing which can be done about this error because Unity does not report
             * the object as being null, as it's still being deconstructed, and there is no way to check
             * if an object is being destroyed.
             * */
            _graphicalTransform.SetParent(_targetTransform);
            _graphicalTransform.SetLocalProperties(_trackerTransform.GetLocalProperties());
        }

        public void ResetState()
        {
            if (!IsInitialized)
                return;

            AttachOnStop();

            _initializingNetworkBehaviour = null;
            _initializingTimeManager = null;
            _graphicalTransform = null;
            _targetTransform = null;

            _teleportedTick = TimeManager.UNSET_TICK;
            _movementMultiplier = 1f;
            CollectionCaches<TickTransformProperties>.StoreAndDefault(ref _transformProperties);
            _moveRates = default;
            _preTicked = default;
            _queuedTrackerProperties = null;
            _trackerPreTickWorldValues = default;
            _graphicsPreTickWorldValues = default;
            _realtimeInterpolation = default;
            _isMoving = default;
            _predictionNetworkTransform = null;
            
            if (_trackerTransform != null)
                UnityEngine.Object.Destroy(_trackerTransform.gameObject);
        }

        public void InitializeState() { }
    }
}
#endif