using FishNet.Managing.Timing;
using UnityEngine;

namespace FishNet.Component.Utility
{
    /// <summary>
    /// Add to any object to display current ping(round trip time).
    /// </summary>
    [AddComponentMenu("FishNet/Component/PingDisplay")]
    public class PingDisplay : MonoBehaviour
    {
        #region Types.
        private enum Corner
        {
            TopLeft,
            TopRight,
            BottomLeft,
            BottomRight
        }
        #endregion

        #region Serialized.
        /// <summary>
        /// Color for text.
        /// </summary>
        [Tooltip("Color for text.")]
        [SerializeField]
        private Color _color = Color.white;
        /// <summary>
        /// Which corner to display ping in.
        /// </summary>
        [Tooltip("Which corner to display ping in.")]
        [SerializeField]
        private Corner _placement = Corner.TopRight;
        /// <summary>
        /// True to show the real ping. False to include tick rate latency within the ping.
        /// </summary>
        [Tooltip("True to show the real ping. False to include tick rate latency within the ping.")]
        [SerializeField]
        private bool _hideTickRate = true;
        #endregion

#if UNITY_EDITOR || !UNITY_SERVER

        #region Private.
        /// <summary>
        /// Style for drawn ping.
        /// </summary>
        private GUIStyle _style = new();
        #endregion

        private void OnGUI()
        {
            // Only clients can see pings.
            if (!InstanceFinder.IsClientStarted)
                return;

            _style.normal.textColor = _color;
            _style.fontSize = 15;
            float width = 85f;
            float height = 15f;
            float edge = 10f;

            float horizontal;
            float vertical;

            if (_placement == Corner.TopLeft)
            {
                horizontal = 10f;
                vertical = 10f;
            }
            else if (_placement == Corner.TopRight)
            {
                horizontal = Screen.width - width - edge;
                vertical = 10f;
            }
            else if (_placement == Corner.BottomLeft)
            {
                horizontal = 10f;
                vertical = Screen.height - height - edge;
            }
            else
            {
                horizontal = Screen.width - width - edge;
                vertical = Screen.height - height - edge;
            }

            long ping;
            TimeManager tm = InstanceFinder.TimeManager;
            if (tm == null)
            {
                ping = 0;
            }
            else
            {
                ping = tm.RoundTripTime;
                long deduction = 0;
                if (_hideTickRate)
                    deduction = (long)(tm.TickDelta * 2000d);

                ping = (long)Mathf.Max(1, ping - deduction);
            }

            GUI.Label(new(horizontal, vertical, width, height), $"Ping: {ping}ms", _style);
        }
#endif
    }
}