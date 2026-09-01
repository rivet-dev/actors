using FishNet.Connection;
using FishNet.Object;
using FishNet.Observing;
using System;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Component.Observing
{
    /// <summary>
    /// When this observer condition is placed on an object, a client must be within the specified distance to view the object.
    /// </summary>
    [CreateAssetMenu(menuName = "FishNet/Observers/Distance Condition", fileName = "New Distance Condition")]
    public class DistanceCondition : ObserverCondition
    {
        #region Serialized.
        /// <summary>
        /// </summary>
        [Tooltip("Maximum distance a client must be within this object to see it.")]
        [SerializeField]
        private float _maximumDistance = 100f;
        /// <summary>
        /// Maximum distance a client must be within this object to see it.
        /// </summary>
        [Obsolete("Use Get/SetMaximumDistance.")]
        public float MaximumDistance
        {
            get => GetMaximumDistance();
            set => SetMaximumDistance(value);
        }

        /// <summary>
        /// Maximum distance a client must be within this object to see it.
        /// </summary>
        /// <returns></returns>
        public float GetMaximumDistance() => _maximumDistance;

        /// <summary>
        /// Sets the maximum distance value.
        /// </summary>
        /// <param name = "value">New value.</param>
        public void SetMaximumDistance(float value)
        {
            _maximumDistance = value;
            _sqrMaximumDistance = _maximumDistance * _maximumDistance;

            float maxDistanceHide = _maximumDistance * (1f + _hideDistancePercent);
            _sqrHideMaximumDistance = maxDistanceHide * maxDistanceHide;
        }

        /// <summary>
        /// Additional percent of distance client must be until this object is hidden. For example, if distance was 100f and percent was 0.5f the client must be 150f units away before this object is hidden again. This can be useful for keeping objects from regularly appearing and disappearing.
        /// </summary>
        [Tooltip("Additional percent of distance client must be until this object is hidden. For example, if distance was 100f and percent was 0.5f the client must be 150f units away before this object is hidden again. This can be useful for keeping objects from regularly appearing and disappearing.")]
        [Range(0f, 1f)]
        [SerializeField]
        private float _hideDistancePercent = 0.1f;
        #endregion

        #region Private.
        /// <summary>
        /// MaximumDistance squared for faster checks.
        /// </summary>
        private float _sqrMaximumDistance;
        /// <summary>
        /// Distance to hide object at.
        /// </summary>
        private float _sqrHideMaximumDistance;
        #endregion

        private void Awake()
        {
            SetMaximumDistance(_maximumDistance);
        }

        /// <summary>
        /// Returns if the object which this condition resides should be visible to connection.
        /// </summary>
        /// <param name = "connection">Connection which the condition is being checked for.</param>
        /// <param name = "currentlyAdded">True if the connection currently has visibility of this object.</param>
        /// <param name = "notProcessed">True if the condition was not processed. This can be used to skip processing for performance. While output as true this condition result assumes the previous ConditionMet value.</param>
        public override bool ConditionMet(NetworkConnection connection, bool currentlyAdded, out bool notProcessed)
        {
            // If here then checks are being processed.
            notProcessed = false;

            float sqrMaximumDistance = currentlyAdded ? _sqrHideMaximumDistance : _sqrMaximumDistance;
            Vector3 thisPosition = NetworkObject.transform.position;
            foreach (NetworkObject nob in connection.Objects)
            {
                // If within distance.
                if (Vector3.SqrMagnitude(nob.transform.position - thisPosition) <= sqrMaximumDistance)
                    return true;
            }

            /* If here no client objects are within distance. */
            return false;
        }

        /// <summary>
        /// Type of condition this is. Certain types are handled different, such as Timed which are checked for changes at timed intervals.
        /// </summary>
        /// <returns></returns>
        public override ObserverConditionType GetConditionType() => ObserverConditionType.Timed;
    }
}