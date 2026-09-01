//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text;

namespace MonoFN.Cecil
{
    public delegate AssemblyDefinition AssemblyResolveEventHandler(object sender, AssemblyNameReference reference);

    public sealed class AssemblyResolveEventArgs : EventArgs
    {
        public AssemblyNameReference AssemblyReference { get; }

        public AssemblyResolveEventArgs(AssemblyNameReference reference)
        {
            this.AssemblyReference = reference;
        }
    }

#if !NET_CORE
    [Serializable]
#endif
    public sealed class AssemblyResolutionException : FileNotFoundException
    {
        public AssemblyNameReference AssemblyReference { get; }
        public AssemblyResolutionException(AssemblyNameReference reference) : this(reference, null) { }

        public AssemblyResolutionException(AssemblyNameReference reference, Exception innerException) : base(string.Format("Failed to resolve assembly: '{0}'", reference), innerException)
        {
            this.AssemblyReference = reference;
        }

#if !NET_CORE
        private AssemblyResolutionException(System.Runtime.Serialization.SerializationInfo info, System.Runtime.Serialization.StreamingContext context) : base(info, context) { }
#endif
    }

    public abstract class BaseAssemblyResolver : IAssemblyResolver
    {
        private static readonly bool on_mono = Type.GetType("MonoFN.Runtime") != null;
        private readonly Collection<string> directories;
#if NET_CORE
	    // Maps file names of available trusted platform assemblies to their full paths.
	    // Internal for testing.
	    internal static readonly Lazy<Dictionary<string, string>> TrustedPlatformAssemblies = new Lazy<Dictionary<string, string>>(CreateTrustedPlatformAssemblyMap);
#else
        private Collection<string> gac_paths;
#endif

        public void AddSearchDirectory(string directory)
        {
            directories.Add(directory);
        }

        public void RemoveSearchDirectory(string directory)
        {
            directories.Remove(directory);
        }

        public string[] GetSearchDirectories()
        {
            string[] directories = new string [this.directories.size];
            Array.Copy(this.directories.items, directories, directories.Length);
            return directories;
        }

        public event AssemblyResolveEventHandler ResolveFailure;

        protected BaseAssemblyResolver()
        {
            directories = new(2) { ".", "bin" };
        }

        private AssemblyDefinition GetAssembly(string file, ReaderParameters parameters)
        {
            if (parameters.AssemblyResolver == null)
                parameters.AssemblyResolver = this;

            return ModuleDefinition.ReadModule(file, parameters).Assembly;
        }

        public virtual AssemblyDefinition Resolve(AssemblyNameReference name)
        {
            return Resolve(name, new());
        }

        public virtual AssemblyDefinition Resolve(AssemblyNameReference name, ReaderParameters parameters)
        {
            Mixin.CheckName(name);
            Mixin.CheckParameters(parameters);

            AssemblyDefinition assembly = SearchDirectory(name, directories, parameters);
            if (assembly != null)
                return assembly;

            if (name.IsRetargetable)
            {
                // if the reference is retargetable, zero it
                name = new(name.Name, Mixin.ZeroVersion)
                {
                    PublicKeyToken = Empty<byte>.Array
                };
            }

#if NET_CORE
	        assembly = SearchTrustedPlatformAssemblies(name, parameters);
	        if (assembly != null)
		        return assembly;
#else
            string framework_dir = Path.GetDirectoryName(typeof(object).Module.FullyQualifiedName);
            string[] framework_dirs = on_mono ? new[] { framework_dir, Path.Combine(framework_dir, "Facades") } : new[] { framework_dir };

            if (IsZero(name.Version))
            {
                assembly = SearchDirectory(name, framework_dirs, parameters);
                if (assembly != null)
                    return assembly;
            }

            if (name.Name == "mscorlib")
            {
                assembly = GetCorlib(name, parameters);
                if (assembly != null)
                    return assembly;
            }

            assembly = GetAssemblyInGac(name, parameters);
            if (assembly != null)
                return assembly;

            assembly = SearchDirectory(name, framework_dirs, parameters);
            if (assembly != null)
                return assembly;
#endif
            if (ResolveFailure != null)
            {
                assembly = ResolveFailure(this, name);
                if (assembly != null)
                    return assembly;
            }

            throw new AssemblyResolutionException(name);
        }

#if NET_CORE
	    AssemblyDefinition SearchTrustedPlatformAssemblies(AssemblyNameReference name, ReaderParameters parameters)
	    {
		    if (name.IsWindowsRuntime)
			    return null;

		    if (TrustedPlatformAssemblies.Value.TryGetValue(name.Name, out string path))
			    return GetAssembly(path, parameters);

		    return null;
	    }

	    static Dictionary<string, string> CreateTrustedPlatformAssemblyMap()
	    {
		    var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

		    string paths;

		    try
		    {
			    paths = (string)AppDomain.CurrentDomain.GetData("TRUSTED_PLATFORM_ASSEMBLIES");
		    }
		    catch
		    {
			    paths = null;
		    }

		    if (paths == null)
			    return result;

		    foreach (var path in paths.Split(Path.PathSeparator))
			    if (string.Equals(Path.GetExtension(path), ".dll", StringComparison.OrdinalIgnoreCase))
				    result[Path.GetFileNameWithoutExtension(path)] = path;

		    return result;
	    }
#endif

        protected virtual AssemblyDefinition SearchDirectory(AssemblyNameReference name, IEnumerable<string> directories, ReaderParameters parameters)
        {
            string[] extensions = name.IsWindowsRuntime ? new[] { ".winmd", ".dll" } : new[] { ".exe", ".dll" };
            foreach (string directory in directories)
            {
                foreach (string extension in extensions)
                {
                    string file = Path.Combine(directory, name.Name + extension);
                    if (!File.Exists(file))
                        continue;
                    try
                    {
                        return GetAssembly(file, parameters);
                    }
                    catch (BadImageFormatException)
                    {
                        continue;
                    }
                }
            }

            return null;
        }

        private static bool IsZero(Version version)
        {
            return version.Major == 0 && version.Minor == 0 && version.Build == 0 && version.Revision == 0;
        }

#if !NET_CORE
        private AssemblyDefinition GetCorlib(AssemblyNameReference reference, ReaderParameters parameters)
        {
            Version version = reference.Version;
            AssemblyName corlib = typeof(object).Assembly.GetName();
            if (corlib.Version == version || IsZero(version))
                return GetAssembly(typeof(object).Module.FullyQualifiedName, parameters);

            string path = Directory.GetParent(Directory.GetParent(typeof(object).Module.FullyQualifiedName).FullName).FullName;

            if (on_mono)
            {
                if (version.Major == 1)
                {
                    path = Path.Combine(path, "1.0");
                }
                else if (version.Major == 2)
                {
                    if (version.MajorRevision == 5)
                        path = Path.Combine(path, "2.1");
                    else
                        path = Path.Combine(path, "2.0");
                }
                else if (version.Major == 4)
                {
                    path = Path.Combine(path, "4.0");
                }
                else
                {
                    throw new NotSupportedException("Version not supported: " + version);
                }
            }
            else
            {
                switch (version.Major)
                {
                    case 1:
                        if (version.MajorRevision == 3300)
                            path = Path.Combine(path, "v1.0.3705");
                        else
                            path = Path.Combine(path, "v1.1.4322");
                        break;
                    case 2:
                        path = Path.Combine(path, "v2.0.50727");
                        break;
                    case 4:
                        path = Path.Combine(path, "v4.0.30319");
                        break;
                    default:
                        throw new NotSupportedException("Version not supported: " + version);
                }
            }

            string file = Path.Combine(path, "mscorlib.dll");
            if (File.Exists(file))
                return GetAssembly(file, parameters);

            if (on_mono && Directory.Exists(path + "-api"))
            {
                file = Path.Combine(path + "-api", "mscorlib.dll");
                if (File.Exists(file))
                    return GetAssembly(file, parameters);
            }

            return null;
        }

        private static Collection<string> GetGacPaths()
        {
            if (on_mono)
                return GetDefaultMonoGacPaths();

            Collection<string> paths = new(2);
            string windir = Environment.GetEnvironmentVariable("WINDIR");
            if (windir == null)
                return paths;

            paths.Add(Path.Combine(windir, "assembly"));
            paths.Add(Path.Combine(windir, Path.Combine("Microsoft.NET", "assembly")));
            return paths;
        }

        private static Collection<string> GetDefaultMonoGacPaths()
        {
            Collection<string> paths = new(1);
            string gac = GetCurrentMonoGac();
            if (gac != null)
                paths.Add(gac);

            string gac_paths_env = Environment.GetEnvironmentVariable("MONO_GAC_PREFIX");
            if (string.IsNullOrEmpty(gac_paths_env))
                return paths;

            string[] prefixes = gac_paths_env.Split(Path.PathSeparator);
            foreach (string prefix in prefixes)
            {
                if (string.IsNullOrEmpty(prefix))
                    continue;

                string gac_path = Path.Combine(Path.Combine(Path.Combine(prefix, "lib"), "mono"), "gac");
                if (Directory.Exists(gac_path) && !paths.Contains(gac))
                    paths.Add(gac_path);
            }

            return paths;
        }

        private static string GetCurrentMonoGac()
        {
            return Path.Combine(Directory.GetParent(Path.GetDirectoryName(typeof(object).Module.FullyQualifiedName)).FullName, "gac");
        }

        private AssemblyDefinition GetAssemblyInGac(AssemblyNameReference reference, ReaderParameters parameters)
        {
            if (reference.PublicKeyToken == null || reference.PublicKeyToken.Length == 0)
                return null;

            if (gac_paths == null)
                gac_paths = GetGacPaths();

            if (on_mono)
                return GetAssemblyInMonoGac(reference, parameters);

            return GetAssemblyInNetGac(reference, parameters);
        }

        private AssemblyDefinition GetAssemblyInMonoGac(AssemblyNameReference reference, ReaderParameters parameters)
        {
            for (int i = 0; i < gac_paths.Count; i++)
            {
                string gac_path = gac_paths[i];
                string file = GetAssemblyFile(reference, string.Empty, gac_path);
                if (File.Exists(file))
                    return GetAssembly(file, parameters);
            }

            return null;
        }

        private AssemblyDefinition GetAssemblyInNetGac(AssemblyNameReference reference, ReaderParameters parameters)
        {
            string[] gacs = new[] { "GAC_MSIL", "GAC_32", "GAC_64", "GAC" };
            string[] prefixes = new[] { string.Empty, "v4.0_" };

            for (int i = 0; i < gac_paths.Count; i++)
            {
                for (int j = 0; j < gacs.Length; j++)
                {
                    string gac = Path.Combine(gac_paths[i], gacs[j]);
                    string file = GetAssemblyFile(reference, prefixes[i], gac);
                    if (Directory.Exists(gac) && File.Exists(file))
                        return GetAssembly(file, parameters);
                }
            }

            return null;
        }

        private static string GetAssemblyFile(AssemblyNameReference reference, string prefix, string gac)
        {
            StringBuilder gac_folder = new StringBuilder().Append(prefix).Append(reference.Version).Append("__");

            for (int i = 0; i < reference.PublicKeyToken.Length; i++)
                gac_folder.Append(reference.PublicKeyToken[i].ToString("x2"));

            return Path.Combine(Path.Combine(Path.Combine(gac, reference.Name), gac_folder.ToString()), reference.Name + ".dll");
        }
#endif
        public void Dispose()
        {
            Dispose(true);
            GC.SuppressFinalize(this);
        }

        protected virtual void Dispose(bool disposing) { }
    }
}