using System;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Object.Prediction
{
    [Obsolete("This class will be removed in version 5.")]
    internal class LocalTransformTickSmoother : IResettable
    {
        #region Private.
        /// <summary>
        /// Object to smooth.
        /// </summary>
        private Transform _graphicalObject;
        /// <summary>
        /// When not MoveRates.UNSET_VALUE the graphical object will teleport into it's next position if the move distance exceeds this value.
        /// </summary>
        private float _teleportThreshold;
        /// <summary>
        /// How quickly to move towards goal values.
        /// </summary>
        private MoveRates _moveRates;
        /// <summary>
        /// True if a pretick occurred since last postTick.
        /// </summary>
        private bool _preTicked;
        /// <summary>
        /// Local values of the graphical during pretick.
        /// </summary>
        private TransformProperties _gfxInitializedLocalValues;
        /// <summary>
        /// World values of the graphical after it's been aligned to initialized values in PreTick.
        /// </summary>
        private TransformProperties _gfxPreSimulateWorldValues;
        /// <summary>
        /// TickDelta on the TimeManager.
        /// </summary>
        private float _tickDelta;
        /// <summary>
        /// How many ticks to interpolate over.
        /// </summary>
        private byte _interpolation;
        #endregion

        /// <summary>
        /// Initializes this smoother; should only be completed once.
        /// </summary>
        internal void InitializeOnce(Transform graphicalObject, float teleportDistance, float tickDelta, byte interpolation)
        {
            _gfxInitializedLocalValues = graphicalObject.GetLocalProperties();
            _tickDelta = tickDelta;
            _graphicalObject = graphicalObject;
            _teleportThreshold = teleportDistance * (float)interpolation;
            _interpolation = interpolation;
        }

        /// <summary>
        /// Called every frame.
        /// </summary>
        internal void Update()
        {
            if (!CanSmooth())
                return;

            MoveToTarget();
        }

        /// <summary>
        /// Called when the TimeManager invokes OnPreTick.
        /// </summary>
        internal void OnPreTick()
        {
            if (!CanSmooth())
                return;

            _preTicked = true;
            _gfxPreSimulateWorldValues = _graphicalObject.GetWorldProperties();
        }

        /// <summary>
        /// Called when TimeManager invokes OnPostTick.
        /// </summary>
        internal void OnPostTick()
        {
            if (!CanSmooth())
                return;

            // If preticked then previous transform values are known.
            if (_preTicked)
            {
                _graphicalObject.SetWorldProperties(_gfxPreSimulateWorldValues);
                SetMoveRates(_gfxInitializedLocalValues, _graphicalObject);
            }
            // If did not pretick then the only thing we can do is snap to instantiated values.
            else
            {
                _graphicalObject.SetLocalProperties(_gfxInitializedLocalValues);
            }
        }

        /// <summary>
        /// Returns if prediction can be used on this rigidbody.
        /// </summary>
        /// <returns></returns>
        private bool CanSmooth()
        {
            if (_graphicalObject == null)
                return false;

            return true;
        }

        /// <summary>
        /// Sets Position and Rotation move rates to reach Target datas.
        /// </summary>
        private void SetMoveRates(TransformProperties prevValues, Transform t)
        {
            float duration = _tickDelta * (float)_interpolation;
            /* If interpolation is 1 then add on a tiny amount
             * of more time to compensate for frame time, so that
             * the smoothing does not complete before the next tick,
             * as this would result in jitter. */
            if (_interpolation == 1)
                duration += Mathf.Max(Time.deltaTime, 1f / 50f);
            float teleportT = _teleportThreshold;
            _moveRates = MoveRates.GetLocalMoveRates(prevValues, t, duration, teleportT);
        }

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        private void MoveToTarget()
        {
            _moveRates.Move(_graphicalObject, _gfxInitializedLocalValues, Time.deltaTime, useWorldSpace: false);
        }

        public void ResetState()
        {
            if (_graphicalObject != null)
            {
                _graphicalObject.SetLocalProperties(_gfxInitializedLocalValues);
                _graphicalObject = null;
            }
            _teleportThreshold = default;
            _moveRates = default;
            _preTicked = default;
            _gfxInitializedLocalValues = default;
            _gfxPreSimulateWorldValues = default;
            _tickDelta = default;
            _interpolation = default;
        }

        public void InitializeState() { }
    }
}