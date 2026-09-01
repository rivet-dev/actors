using FishNet.Managing;
using FishNet.Managing.Logging;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Object.Prediction;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using Unity.Profiling;
using UnityEngine;

namespace FishNet.Component.Transforming
{
    /// <summary>
    /// Detaches the object which this component resides and follows another.
    /// </summary>
    public class DetachableNetworkTickSmoother : NetworkBehaviour
    {
        #region Serialized.
        /// <summary>
        /// True to attach the object to it's original parent when OnStopClient is called.
        /// </summary>
        [Tooltip("True to attach the object to it's original parent when OnStopClient is called.")]
        [SerializeField]
        private bool _attachOnStop = true;
        /// <summary>
        /// Object to follow, and smooth towards.
        /// </summary>
        [Tooltip("Object to follow, and smooth towards.")]
        [SerializeField]
        private Transform _followObject;
        /// <summary>
        /// How many ticks to interpolate over.
        /// </summary>
        [Tooltip("How many ticks to interpolate over.")]
        [Range(1, byte.MaxValue)]
        [SerializeField]
        private byte _interpolation = 1;
        /// <summary>
        /// True to enable teleport threshhold.
        /// </summary>
        [Tooltip("True to enable teleport threshold.")]
        [SerializeField]
        private bool _enableTeleport;
        /// <summary>
        /// How far the object must move between ticks to teleport rather than smooth.
        /// </summary>
        [Tooltip("How far the object must move between ticks to teleport rather than smooth.")]
        [Range(0f, ushort.MaxValue)]
        [SerializeField]
        private float _teleportThreshold;
        /// <summary>
        /// True to synchronize the position of the followObject.
        /// </summary>
        [Tooltip("True to synchronize the position of the followObject.")]
        [SerializeField]
        private bool _synchronizePosition = true;
        /// <summary>
        /// True to synchronize the rotation of the followObject.
        /// </summary>
        [Tooltip("True to synchronize the rotation of the followObject.")]
        [SerializeField]
        private bool _synchronizeRotation;
        /// <summary>
        /// True to synchronize the scale of the followObject.
        /// </summary>
        [Tooltip("True to synchronize the scale of the followObject.")]
        [SerializeField]
        private bool _synchronizeScale;
        #endregion

        #region Private.
        /// <summary>
        /// TimeManager subscribed to.
        /// </summary>
        private TimeManager _timeManager;
        /// <summary>
        /// Parent of the object prior to detaching.
        /// </summary>
        private Transform _parent;
        /// <summary>
        /// Local properties of the graphical during instantation.
        /// </summary>
        private TransformProperties _transformInstantiatedLocalProperties;
        /// <summary>
        /// World properties of the followObject during  post tick.
        /// </summary>
        private TransformProperties _postTickFollowObjectWorldProperties;
        /// <summary>
        /// How quickly to move towards target.
        /// </summary>
        private MoveRates _moveRates = new(MoveRates.INSTANT_VALUE);
        /// <summary>
        /// True if initialized.
        /// </summary>
        private bool _initialized;
        /// <summary>
        /// Cached TickDelta of the TimeManager.
        /// </summary>
        private float _tickDelta;
        
        private static readonly ProfilerMarker _pm_OnPostTick = new("DetachableNetworkTickSmoother._timeManager_OnPostTick()");
        #endregion

        private void Awake()
        {
            _transformInstantiatedLocalProperties = transform.GetLocalProperties();
        }

        private void OnDestroy()
        {
            ChangeSubscription(false);
        }

        public override void OnStartClient()
        {
            bool error = false;
            if (transform.parent == null)
            {
                NetworkManager.LogError($"{GetType().Name} on gameObject {gameObject.name} requires a parent to detach from.");
                error = true;
            }
            if (_followObject == null)
            {
                NetworkManager.LogError($"{GetType().Name} on gameObject {gameObject}, root {transform.root} requires followObject to be set.");
                error = true;
            }

            if (error)
                return;

            _parent = transform.parent;
            transform.SetParent(null);

            SetTimeManager(TimeManager);
            // Unsub first in the rare chance we already subbed such as a stop callback issue.
            ChangeSubscription(false);
            ChangeSubscription(true);

            _postTickFollowObjectWorldProperties = _followObject.GetWorldProperties();
            _tickDelta = (float)TimeManager.TickDelta;
            _initialized = true;
        }

        public override void OnStopClient()
        {
#if UNITY_EDITOR
            if (ApplicationState.IsQuitting())
                return;
#endif
            // Reattach to parent.
            if (_attachOnStop && _parent != null)
            {
                // Reparent
                transform.SetParent(_parent);
                // Set to instantiated local values.
                transform.SetLocalProperties(_transformInstantiatedLocalProperties);
            }

            _postTickFollowObjectWorldProperties.ResetState();
            ChangeSubscription(false);

            _initialized = false;
        }

        [Client(Logging = LoggingType.Off)]
        private void Update()
        {
            MoveTowardsFollowTarget();
        }

        /// <summary>
        /// Called after a tick completes.
        /// </summary>
        private void _timeManager_OnPostTick()
        {
            using (_pm_OnPostTick.Auto())
            {
                if (!_initialized)
                    return;

                _postTickFollowObjectWorldProperties.Update(_followObject);
                // Unset values if not following the transform property.
                if (!_synchronizePosition)
                    _postTickFollowObjectWorldProperties.Position = transform.position;
                if (!_synchronizeRotation)
                    _postTickFollowObjectWorldProperties.Rotation = transform.rotation;
                if (!_synchronizeScale)
                    _postTickFollowObjectWorldProperties.Scale = transform.localScale;
                SetMoveRates();
            }
        }

        /// <summary>
        /// Sets a new PredictionManager to use.
        /// </summary>
        /// <param name = "tm"></param>
        private void SetTimeManager(TimeManager tm)
        {
            if (tm == _timeManager)
                return;

            // Unsub from current.
            ChangeSubscription(false);
            // Sub to newest.
            _timeManager = tm;
            ChangeSubscription(true);
        }

        /// <summary>
        /// Changes the subscription to the TimeManager.
        /// </summary>
        private void ChangeSubscription(bool subscribe)
        {
            if (_timeManager == null)
                return;

            if (subscribe)
                _timeManager.OnPostTick += _timeManager_OnPostTick;
            else
                _timeManager.OnPostTick -= _timeManager_OnPostTick;
        }

        /// <summary>
        /// Moves towards targetObject.
        /// </summary>
        private void MoveTowardsFollowTarget()
        {
            if (!_initialized)
                return;

            _moveRates.Move(transform, _postTickFollowObjectWorldProperties, Time.deltaTime, useWorldSpace: true);
        }

        private void SetMoveRates()
        {
            if (!_initialized)
                return;

            float duration = _tickDelta * _interpolation;
            /* If interpolation is 1 then add on a tiny amount
             * of more time to compensate for frame time, so that
             * the smoothing does not complete before the next tick,
             * as this would result in jitter. */
            if (_interpolation == 1)
                duration += Mathf.Max(Time.deltaTime, 1f / 50f);

            float teleportT = _enableTeleport ? _teleportThreshold : MoveRates.UNSET_VALUE;
            _moveRates = MoveRates.GetWorldMoveRates(transform, _followObject, duration, teleportT);
        }
    }
}