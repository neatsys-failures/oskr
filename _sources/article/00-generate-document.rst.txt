============================================
Meta-instruction on generating this document
============================================

The document is built in two steps: 

* Use Doxygen to extract code annotations from source.
* Use Sphinx to merge Doxygen's output with other documents (with the help of
  Breathe extension), and produce final assets.

Sphinx is avialable as apt pachage ``python3-sphinx``, while two other pypi 
packages are required: ``furo`` as Sphinx theme, and ``m2r2`` as markdown 
convertor.

For Doxygen and Breathe, they have to be built from source, because they only
support C++20 features in this project very recently. The following apt packages
are required: ``libllvm-13-dev``, ``libclang-13-dev``. Their own source are
submodule of this project located in ``dependency``.

Build Doxygen as normal CMake project, with ``-Duse_clang=ON`` option.

Build and install Breathe as normal pypi project.

After setting them up, from project directory run:

.. code-block:: bash

    ./dependency/doxygen/build/bin/doxygen
    sphinx-build -M html doc doc/_build

The built document (identical to gh-pages branch content) is located in 
``doc/_build/html``.
