#if UNITY_EDITOR
using FishNet.Configuring;
using System;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;
using File = System.IO.File;

namespace FishNet.Editing.NewNetworkBehaviourScript
{
    internal sealed class CreateNewNetworkBehaviour : MonoBehaviour
    {
        private const string TEMPLATE_CLASS_NAME = "NewNetworkBehaviourTemplate";
        public static string TemplatePath => Path.Combine(Configuration.Configurations.CreateNewNetworkBehaviour.templateDirectoryPath, $"{TEMPLATE_CLASS_NAME}.txt");

        [MenuItem("Assets/Create/FishNet/NetworkBehaviour Script", false, -220)]
        private static void CreateNewAsset()
        {
            try
            {
                EnsureTemplateExists();
                ProjectWindowUtil.CreateScriptAssetFromTemplateFile(TemplatePath, $"{TEMPLATE_CLASS_NAME}.cs");
            }
            catch (Exception e)
            {
                Debug.LogError($"An exception occurred while trying to copy the NetworkBehaviour template. {e.Message}");
            }
        }

        public static void EnsureTemplateExists()
        {
            try
            {
                if (!File.Exists(TemplatePath))
                {
                    string fileContent = GetNewTemplateText();
                    File.WriteAllText(TemplatePath, fileContent);
                }
            }
            catch (Exception e)
            {
                Debug.LogError($"An exception occurred while trying to create the NetworkBehaviour template. {e.Message}");
            }
        }

        private static string GetNewTemplateText()
        {
            StringBuilder sb = new();
            sb.AppendLine("using UnityEngine;");
            sb.AppendLine("using FishNet.Object;");
            sb.AppendLine();
            sb.AppendLine("public class NewNetworkBehaviourTemplate : NetworkBehaviour");
            sb.AppendLine("{");
            sb.AppendLine("     private void Awake() { }");
            sb.AppendLine();
            sb.AppendLine("     private void Update() { }");
            sb.AppendLine("}");

            return sb.ToString();
        }
    }
}
#endif