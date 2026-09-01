using MonoFN.Cecil;
using System.Linq;
using System.Reflection;

namespace FishNet.CodeGenerating.ILCore
{
    internal class FNPostProcessorReflectionImporter : DefaultReflectionImporter
    {
        private const string k_SystemPrivateCoreLib = "System.Private.CoreLib";
        private readonly AssemblyNameReference m_CorrectCorlib;

        public FNPostProcessorReflectionImporter(ModuleDefinition module) : base(module)
        {
            m_CorrectCorlib = module.AssemblyReferences.FirstOrDefault(a => a.Name == "mscorlib" || a.Name == "netstandard" || a.Name == k_SystemPrivateCoreLib);
        }

        public override AssemblyNameReference ImportReference(AssemblyName reference)
        {
            return m_CorrectCorlib != null && reference.Name == k_SystemPrivateCoreLib ? m_CorrectCorlib : base.ImportReference(reference);
        }
    }
}