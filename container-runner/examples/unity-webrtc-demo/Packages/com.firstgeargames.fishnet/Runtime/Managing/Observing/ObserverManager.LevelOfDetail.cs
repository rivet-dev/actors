using System;
using System.Collections.Generic;
using FishNet.Connection;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Utility;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Managing.Observing
{
    /// <summary>
    /// Handles level of detail actions.
    /// </summary>
    public sealed partial class ObserverManager : MonoBehaviour
    {
        /// <summary>
        /// Most recent LocalTick value on the TimeManager.
        /// </summary>
        internal uint LocalTick;

        [Tooltip("True to enable level of detail.")]
        [SerializeField]
        private bool _useLevelOfDetail;
        /// <summary>
        /// The maximum delay between updates when an object is using the highest level of detail.
        /// </summary>
        [Tooltip("The maximum delay between updates when an object is using the highest level of detail.")]
        [Range(MINIMUM_LEVEL_OF_DETAIL_SEND_INTERVAL, MAXIMUM_LEVEL_OF_DETAIL_SEND_INTERVAL)]
        [SerializeField]
        private float _maximumLevelOfDetailInterval = 2f;
        /// <summary>
        /// The time it will take to calculate new level of detail values. 
        /// </summary>
        [Tooltip("The time it will take to calculate new level of detail values.")]
        [Range(MINIMUM_LEVEL_OF_DETAIL_UPDATE_DURATION, MAXIMUM_LEVEL_OF_DETAIL_UPDATE_DURATION)]
        [SerializeField]
        private float _levelOfDetailUpdateDuration = 1f;
        /// <summary>
        /// Distances for each level of detail change. Each distance is an exponential increase. When the largest distance is surpassed the maximum delay is used; while within the first distance standard delays are used. 
        /// </summary>
        [Tooltip("Distances for each level of detail change. Each distance is an exponential increase. When the largest distance is surpassed the maximum delay is used; while within the first distance standard delays are used.")]
        [SerializeField]
        private List<float> _levelOfDetailDistances = new();

        #region Consts.
        /// <summary>
        /// Minimum time allowed for the maximum level of detail send interval.
        /// </summary>
        private const float MINIMUM_LEVEL_OF_DETAIL_SEND_INTERVAL = 0.1f;
        /// <summary>
        /// Maximum time allowed for the maximum level of detail send interval.
        /// </summary>
        private const float MAXIMUM_LEVEL_OF_DETAIL_SEND_INTERVAL = 15f;
        /// <summary>
        /// Minimum time which can be used for the level of detail update duration.
        /// </summary>
        private const float MINIMUM_LEVEL_OF_DETAIL_UPDATE_DURATION = 0.5f;
        /// <summary>
        /// Maximum time which can be used for the level of detail update duration.
        /// </summary>
        private const float MAXIMUM_LEVEL_OF_DETAIL_UPDATE_DURATION = 10f;
        #endregion

        /// <summary>
        /// Initializes for level of detail use.
        /// </summary>
        /// <returns>New UseLevelOfDetail value.</returns>
        private bool InitializeLevelOfDetailValues()
        {

            return false;
        }
        
        /// <summary>
        /// Updates the duration of how long level of update values should be recalculated.
        /// </summary>
        public void SetLevelOfDetailRecalculationDuration(float duration) => _levelOfDetailUpdateDuration = Mathf.Clamp(duration, MINIMUM_LEVEL_OF_DETAIL_UPDATE_DURATION, MAXIMUM_LEVEL_OF_DETAIL_UPDATE_DURATION);

    }
}