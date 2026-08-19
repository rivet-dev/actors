#if UNITY_EDITOR
using System.Collections.Generic;
using SynapseSocket.Core.Configuration;
using UnityEditor;
using UnityEngine;

namespace FishNet.Transporting.Synapse.Editing
{

    [CustomPropertyDrawer(typeof(ConnectionConfig))]
    public class ConnectionConfigDrawer : PropertyDrawer
    {
        public override float GetPropertyHeight(SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            return lineHeight * 3f;
        }

        public override void OnGUI(Rect position, SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            Rect rect = new(position.x, position.y, position.width, EditorGUIUtility.singleLineHeight);

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("KeepAliveIntervalMilliseconds"), new GUIContent("Keep Alive Interval", "How often keep-alive packets are sent (ms)."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("TimeoutMilliseconds"), new GUIContent("Timeout", "Time without a response before a connection is considered lost (ms)."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("SweepWindowMilliseconds"), new GUIContent("Sweep Window", "Interval for sweeping stale connection state (ms)."));
        }
    }

    [CustomPropertyDrawer(typeof(ReliableConfig))]
    public class ReliableConfigDrawer : PropertyDrawer
    {
        public override float GetPropertyHeight(SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            float height = lineHeight * 4f;

            SerializedProperty ackBatching = property.FindPropertyRelative("AckBatchingEnabled");

            if (ackBatching != null && ackBatching.boolValue)
                height += lineHeight;

            return height;
        }

        public override void OnGUI(Rect position, SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            Rect rect = new(position.x, position.y, position.width, EditorGUIUtility.singleLineHeight);

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumPending"), new GUIContent("Max Pending", "Maximum number of unacknowledged reliable packets before the connection stalls."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("ResendMilliseconds"), new GUIContent("Resend Interval", "Time to wait before resending an unacknowledged reliable packet (ms)."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumRetries"), new GUIContent("Max Retries", "Maximum number of resend attempts before the connection is dropped."));
            rect.y += lineHeight;

            SerializedProperty ackBatching = property.FindPropertyRelative("AckBatchingEnabled");
            EditorGUI.PropertyField(rect, ackBatching, new GUIContent("Ack Batching", "Batch multiple acknowledgements into a single packet to reduce overhead."));
            rect.y += lineHeight;

            if (ackBatching != null && ackBatching.boolValue)
            {
                EditorGUI.indentLevel++;
                EditorGUI.PropertyField(rect, property.FindPropertyRelative("AckBatchIntervalMilliseconds"), new GUIContent("Ack Batch Interval", "How long to wait before flushing a batched acknowledgement (ms)."));
                EditorGUI.indentLevel--;
            }
        }
    }

    [CustomPropertyDrawer(typeof(LatencySimulatorConfig))]
    public class LatencySimulatorConfigDrawer : PropertyDrawer
    {
        public override float GetPropertyHeight(SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            float height = lineHeight;

            SerializedProperty isEnabled = property.FindPropertyRelative("IsEnabled");

            if (isEnabled != null && isEnabled.boolValue)
                height += lineHeight * 5f;

            return height;
        }

        public override void OnGUI(Rect position, SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            Rect rect = new(position.x, position.y, position.width, EditorGUIUtility.singleLineHeight);

            SerializedProperty isEnabled = property.FindPropertyRelative("IsEnabled");
            EditorGUI.PropertyField(rect, isEnabled, new GUIContent("Is Enabled", "Enable the latency simulator to test under simulated network conditions."));
            rect.y += lineHeight;

            if (isEnabled != null && isEnabled.boolValue)
            {
                EditorGUI.indentLevel++;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("BaseLatencyMilliseconds"), new GUIContent("Base Latency", "Fixed delay added to all packets (ms)."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("JitterMilliseconds"), new GUIContent("Jitter", "Random delay variation added on top of base latency (ms)."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("PacketLossChance"), new GUIContent("Loss Chance", "Probability that a packet is dropped (0–1)."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("ReorderChance"), new GUIContent("Reorder Chance", "Probability that a packet is delivered out of order (0–1)."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("OutOfOrderExtraDelayMilliseconds"), new GUIContent("OOO Delay", "Extra delay applied to reordered packets (ms)."));

                EditorGUI.indentLevel--;
            }
        }
    }

    [CustomPropertyDrawer(typeof(NatTraversalConfig))]
    public class NatTraversalConfigDrawer : PropertyDrawer
    {
        public override float GetPropertyHeight(SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            float height = lineHeight;

            SerializedProperty mode = property.FindPropertyRelative("Mode");

            if (mode != null && mode.enumValueIndex != 0)
            {
                height += lineHeight * 3f;

                SerializedProperty fullCone = property.FindPropertyRelative("FullCone");

                if (fullCone != null)
                    height += EditorGUI.GetPropertyHeight(fullCone, true);
            }

            return height;
        }

        public override void OnGUI(Rect position, SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            Rect rect = new(position.x, position.y, position.width, EditorGUIUtility.singleLineHeight);

            SerializedProperty mode = property.FindPropertyRelative("Mode");
            EditorGUI.PropertyField(rect, mode, new GUIContent("Mode", "NAT traversal strategy to use when connecting through a NAT."));
            rect.y += lineHeight;

            if (mode != null && mode.enumValueIndex != 0)
            {
                EditorGUI.indentLevel++;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("ProbeCount"), new GUIContent("Probe Count", "Number of probe packets sent per traversal attempt."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("IntervalMilliseconds"), new GUIContent("Interval", "Time between probe packets (ms)."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumAttempts"), new GUIContent("Max Attempts", "Maximum number of traversal attempts before giving up."));
                rect.y += lineHeight;

                SerializedProperty fullCone = property.FindPropertyRelative("FullCone");

                if (fullCone != null)
                {
                    float fullConeHeight = EditorGUI.GetPropertyHeight(fullCone, true);
                    rect.height = fullConeHeight;
                    EditorGUI.PropertyField(rect, fullCone, new GUIContent("Full Cone", "Settings for full-cone NAT traversal."), true);
                    rect.height = EditorGUIUtility.singleLineHeight;
                }

                EditorGUI.indentLevel--;
            }
        }
    }

    [CustomPropertyDrawer(typeof(SecurityConfig))]
    public class SecurityConfigDrawer : PropertyDrawer
    {
        public override float GetPropertyHeight(SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            return lineHeight * 8f;
        }

        public override void OnGUI(Rect position, SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            Rect rect = new(position.x, position.y, position.width, EditorGUIUtility.singleLineHeight);

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("SignatureProviderAsset"), new GUIContent("Sig Provider", "Asset that generates packet signatures for outgoing packets."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("SignatureValidatorAsset"), new GUIContent("Sig Validator", "Asset that validates packet signatures on incoming packets."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumPacketsPerSecond"), new GUIContent("Max Packets/Sec", "Maximum packets allowed per second per connection before triggering a violation."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumBytesPerSecond"), new GUIContent("Max Bytes/Sec", "Maximum bytes allowed per second per connection before triggering a violation."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumOutOfOrderReliablePackets"), new GUIContent("Max OOO Reliable", "Maximum out-of-order reliable packets buffered before the connection is dropped."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumReassembledPacketSize"), new GUIContent("Max Reassembled Size", "Maximum size in bytes of a fully reassembled segmented packet."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("AllowUnknownPackets"), new GUIContent("Allow Unknown", "Allow packets with unrecognized types instead of dropping them."));
            rect.y += lineHeight;

            EditorGUI.PropertyField(rect, property.FindPropertyRelative("DisableHandshakeReplayProtection"), new GUIContent("Disable Replay Protection", "Disable handshake replay protection. Not recommended in production."));
        }
    }

    [CustomPropertyDrawer(typeof(SynapseConfig))]
    public class SynapseConfigDrawer : PropertyDrawer
    {
        private static readonly Dictionary<string, bool> _foldouts = new();

        private static bool GetFoldout(string key, bool defaultExpanded = true)
        {
            if (!_foldouts.TryGetValue(key, out bool expanded))
            {
                expanded = defaultExpanded;
                _foldouts[key] = expanded;
            }

            return expanded;
        }

        private static bool DrawFoldout(Rect rect, string key, string label)
        {
            bool current = GetFoldout(key);
            bool next = EditorGUI.Foldout(rect, current, label, true, EditorStyles.foldoutHeader);

            if (next != current)
                _foldouts[key] = next;

            return next;
        }

        public override float GetPropertyHeight(SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            float height = 0f;

            // Performance
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".perf"))
                height += lineHeight * 4f;

            // Segmentation
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".seg"))
            {
                height += lineHeight;

                SerializedProperty segmentMode = property.FindPropertyRelative("UnreliableSegmentMode");

                if (segmentMode != null && segmentMode.enumValueIndex != 0)
                    height += lineHeight * 3f;
            }

            // Reliability
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".rel"))
            {
                SerializedProperty reliable = property.FindPropertyRelative("Reliable");

                if (reliable != null)
                    height += EditorGUI.GetPropertyHeight(reliable, false);
            }

            // Connection
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".con"))
            {
                SerializedProperty connection = property.FindPropertyRelative("Connection");

                if (connection != null)
                    height += EditorGUI.GetPropertyHeight(connection, false);
            }

            // Misc
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".misc"))
                height += lineHeight * 2f;

            // Security
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".sec"))
            {
                SerializedProperty security = property.FindPropertyRelative("Security");

                if (security != null)
                    height += EditorGUI.GetPropertyHeight(security, false);
            }

            // Latency Simulator
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".lat"))
            {
                SerializedProperty latencySimulator = property.FindPropertyRelative("LatencySimulator");

                if (latencySimulator != null)
                    height += EditorGUI.GetPropertyHeight(latencySimulator, false);
            }

            // NAT Traversal
            height += lineHeight;

            if (GetFoldout(property.propertyPath + ".nat"))
            {
                SerializedProperty natTraversal = property.FindPropertyRelative("NatTraversal");

                if (natTraversal != null)
                    height += EditorGUI.GetPropertyHeight(natTraversal, false);
            }

            return height;
        }

        public override void OnGUI(Rect position, SerializedProperty property, GUIContent label)
        {
            float lineHeight = EditorGUIUtility.singleLineHeight + EditorGUIUtility.standardVerticalSpacing;
            Rect rect = new(position.x, position.y, position.width, EditorGUIUtility.singleLineHeight);

            // Performance
            bool perfExpanded = DrawFoldout(rect, property.propertyPath + ".perf", "Performance");
            rect.y += lineHeight;

            if (perfExpanded)
            {
                EditorGUI.indentLevel++;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumPacketSize"), new GUIContent("Max Packet Size", "Maximum size in bytes of a single outgoing packet before it is segmented."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumTransmissionUnit"), new GUIContent("MTU", "Maximum transmission unit reported to the upper layer for packet sizing decisions."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("SocketReceiveBufferBytes"), new GUIContent("Recv Buffer", "UDP socket receive buffer size in bytes."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("SocketSendBufferBytes"), new GUIContent("Send Buffer", "UDP socket send buffer size in bytes."));
                rect.y += lineHeight;

                EditorGUI.indentLevel--;
            }

            // Segmentation
            bool segExpanded = DrawFoldout(rect, property.propertyPath + ".seg", "Segmentation");
            rect.y += lineHeight;

            if (segExpanded)
            {
                EditorGUI.indentLevel++;

                SerializedProperty segmentMode = property.FindPropertyRelative("UnreliableSegmentMode");
                EditorGUI.PropertyField(rect, segmentMode, new GUIContent("Segment Mode", "Controls how large unreliable packets are split across multiple datagrams."));
                rect.y += lineHeight;

                if (segmentMode != null && segmentMode.enumValueIndex != 0)
                {
                    EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumSegments"), new GUIContent("Max Segments", "Maximum number of fragments a single packet may be split into."));
                    rect.y += lineHeight;

                    EditorGUI.PropertyField(rect, property.FindPropertyRelative("MaximumConcurrentSegmentAssembliesPerConnection"), new GUIContent("Max Concurrent Assemblies", "Maximum number of in-progress segment assemblies allowed per connection."));
                    rect.y += lineHeight;

                    EditorGUI.PropertyField(rect, property.FindPropertyRelative("SegmentAssemblyTimeoutMilliseconds"), new GUIContent("Assembly Timeout", "Time before an incomplete segment assembly is discarded (ms)."));
                    rect.y += lineHeight;
                }

                EditorGUI.indentLevel--;
            }

            // Reliability
            bool relExpanded = DrawFoldout(rect, property.propertyPath + ".rel", "Reliability");
            rect.y += lineHeight;

            if (relExpanded)
            {
                EditorGUI.indentLevel++;

                SerializedProperty reliable = property.FindPropertyRelative("Reliable");

                if (reliable != null)
                {
                    float reliableHeight = EditorGUI.GetPropertyHeight(reliable, false);
                    rect.height = reliableHeight;
                    EditorGUI.PropertyField(rect, reliable, false);
                    rect.height = EditorGUIUtility.singleLineHeight;
                    rect.y += reliableHeight;
                }

                EditorGUI.indentLevel--;
            }

            // Connection
            bool conExpanded = DrawFoldout(rect, property.propertyPath + ".con", "Connection");
            rect.y += lineHeight;

            if (conExpanded)
            {
                EditorGUI.indentLevel++;

                SerializedProperty connection = property.FindPropertyRelative("Connection");

                if (connection != null)
                {
                    float connectionHeight = EditorGUI.GetPropertyHeight(connection, false);
                    rect.height = connectionHeight;
                    EditorGUI.PropertyField(rect, connection, false);
                    rect.height = EditorGUIUtility.singleLineHeight;
                    rect.y += connectionHeight;
                }

                EditorGUI.indentLevel--;
            }

            // Misc
            bool miscExpanded = DrawFoldout(rect, property.propertyPath + ".misc", "Misc");
            rect.y += lineHeight;

            if (miscExpanded)
            {
                EditorGUI.indentLevel++;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("CopyReceivedPayloads"), new GUIContent("Copy Payloads", "Copy received payloads before delivering them, allowing safe access after the callback returns."));
                rect.y += lineHeight;

                EditorGUI.PropertyField(rect, property.FindPropertyRelative("EnableTelemetry"), new GUIContent("Enable Telemetry", "Collect and expose internal transport metrics."));
                rect.y += lineHeight;

                EditorGUI.indentLevel--;
            }

            // Security
            bool secExpanded = DrawFoldout(rect, property.propertyPath + ".sec", "Security");
            rect.y += lineHeight;

            if (secExpanded)
            {
                EditorGUI.indentLevel++;

                SerializedProperty security = property.FindPropertyRelative("Security");

                if (security != null)
                {
                    float securityHeight = EditorGUI.GetPropertyHeight(security, false);
                    rect.height = securityHeight;
                    EditorGUI.PropertyField(rect, security, false);
                    rect.height = EditorGUIUtility.singleLineHeight;
                    rect.y += securityHeight;
                }

                EditorGUI.indentLevel--;
            }

            // Latency Simulator
            bool latExpanded = DrawFoldout(rect, property.propertyPath + ".lat", "Latency Simulator");
            rect.y += lineHeight;

            if (latExpanded)
            {
                EditorGUI.indentLevel++;

                SerializedProperty latencySimulator = property.FindPropertyRelative("LatencySimulator");

                if (latencySimulator != null)
                {
                    float latencyHeight = EditorGUI.GetPropertyHeight(latencySimulator, false);
                    rect.height = latencyHeight;
                    EditorGUI.PropertyField(rect, latencySimulator, false);
                    rect.height = EditorGUIUtility.singleLineHeight;
                    rect.y += latencyHeight;
                }

                EditorGUI.indentLevel--;
            }

            // NAT Traversal
            bool natExpanded = DrawFoldout(rect, property.propertyPath + ".nat", "NAT Traversal");
            rect.y += lineHeight;

            if (natExpanded)
            {
                EditorGUI.indentLevel++;

                SerializedProperty natTraversal = property.FindPropertyRelative("NatTraversal");

                if (natTraversal != null)
                {
                    float natHeight = EditorGUI.GetPropertyHeight(natTraversal, false);
                    rect.height = natHeight;
                    EditorGUI.PropertyField(rect, natTraversal, false);
                    rect.height = EditorGUIUtility.singleLineHeight;
                    rect.y += natHeight;
                }

                EditorGUI.indentLevel--;
            }
        }
    }
}
#endif
