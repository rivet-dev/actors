using UnityEngine;

namespace FishNet.Managing.Statistic
{
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Manager/StatisticsManager")]
    public class StatisticsManager : MonoBehaviour
    {
        /// <summary>
        /// True to operate while in release. This may cause allocations and impact performance.
        /// </summary>
        [Tooltip("True to operate while in release. This may cause allocations and impact performance.")]
        [SerializeField]
        private bool _runInRelease;
        /// <summary>
        /// Statistics for NetworkTraffic.
        /// </summary>
        [Tooltip("Statistics for NetworkTraffic.")]
        [SerializeField]
        private NetworkTrafficStatistics _networkTraffic;
        /// <summary>
        /// NetworkManager this is for.
        /// </summary>
        private NetworkManager _networkManager;

        internal void InitializeOnce_Internal(NetworkManager manager)
        {
            _networkManager = manager;
        }

        /// <summary>
        /// Gets NetworkTrafficStatistics reference.
        /// </summary>
        public bool TryGetNetworkTrafficStatistics(out NetworkTrafficStatistics statistics)
        {
            statistics = null;

            /* Cannot run in the current build type. */
            #if (!UNITY_EDITOR && !DEVELOPMENT_BUILD) || UNITY_SERVER
            if (!_runInRelease)
            {
                _networkTraffic = null;
                return false;
            }
            #endif

            //NetworkManager must be set to work.            
            if (_networkManager == null)
            {
                if (!TryGetComponent(out _networkManager))
                    return false;
            }

            //Hot-load if needed.
            if (_networkTraffic == null)
                _networkTraffic = new();

            _networkTraffic.InitializeOnce_Internal(_networkManager);

            if (_networkTraffic.IsEnabled())
                statistics = _networkTraffic;
            
            return statistics != null;
        }
    }
}